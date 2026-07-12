//! Public `processus` provider.

use faber::Valor;
use host_kernel::{
    DispatchContext, HostError, HostResult, Kernel, Provider, ProviderRegistration, ProviderReply,
    RequestFrame,
};
use std::collections::BTreeMap;
use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub struct Processus {
    registration: ProviderRegistration,
}

impl Processus {
    pub fn new() -> HostResult<Self> {
        Ok(Self {
            registration: ProviderRegistration::new(host_kernel::parse_manifest(manifest_json())?),
        })
    }
}

pub fn register(kernel: &mut Kernel) -> HostResult<()> {
    kernel.register(Arc::new(Processus::new()?))
}

pub fn manifest_json() -> &'static str {
    include_str!("manifest.json")
}

impl Provider for Processus {
    fn registration(&self) -> &ProviderRegistration {
        &self.registration
    }

    fn dispatch(
        &self,
        request: &RequestFrame,
        context: &DispatchContext,
    ) -> HostResult<ProviderReply> {
        match request.route.as_str() {
            "processus:exsequi" | "processus:exsequetur" => execute_shell(&request.opener, context),
            "processus:dimitte" => spawn_detached(&request.opener),
            "processus:lege" => read_env(&request.opener),
            "processus:scribe" => write_env(&request.opener),
            "processus:sedes" => current_dir(),
            "processus:muta" => set_current_dir(&request.opener),
            "processus:identitas" => Ok(ProviderReply::item(Valor::Numerus(
                std::process::id() as i64
            ))),
            "processus:argumenta" => Ok(ProviderReply::list(
                std::env::args().skip(1).map(Valor::Textus),
            )),
            "processus:exi" => exit_process(&request.opener),
            "processus:captura" => capture_process(&request.opener, context),
            other => Err(HostError::no_route(format!(
                "no built-in processus syscall registered for {other}"
            ))),
        }
    }
}

fn execute_shell(opener: &Valor, context: &DispatchContext) -> HostResult<ProviderReply> {
    let command = string_arg(opener, 0, "imperium")?;
    let mut process = Command::new("sh");
    process.arg("-c").arg(command);
    let output = run_command(process, context, "processus:exsequi")?;
    Ok(ProviderReply::item(Valor::Textus(
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )))
}

fn capture_process(opener: &Valor, context: &DispatchContext) -> HostResult<ProviderReply> {
    let args = string_list_arg(opener, 0, "args")?;
    let (program, program_args) = args.split_first().ok_or_else(|| {
        HostError::invalid_args("processus:captura requires a non-empty args list")
    })?;
    let mut process = Command::new(program);
    process.args(program_args);
    let output = run_command(process, context, "processus:captura")?;
    let mut fields = BTreeMap::new();
    fields.insert(
        "status".to_owned(),
        Valor::Numerus(output.status.code().map_or(-1, i64::from)),
    );
    fields.insert(
        "stdout".to_owned(),
        Valor::Textus(String::from_utf8_lossy(&output.stdout).into_owned()),
    );
    fields.insert(
        "stderr".to_owned(),
        Valor::Textus(String::from_utf8_lossy(&output.stderr).into_owned()),
    );
    Ok(ProviderReply::item(Valor::Tabula(fields)))
}

struct CommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run_command(
    mut command: Command,
    context: &DispatchContext,
    operation: &str,
) -> HostResult<CommandOutput> {
    if context.cancellation.is_cancelled() {
        return Err(HostError::cancelled());
    }

    configure_process_group(&mut command);
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| HostError::internal(format!("{operation} failed: {error}")))?;
    let stdout = match child.stdout.take() {
        Some(pipe) => pipe,
        None => return Err(pipe_unavailable(&mut child, operation, "stdout")),
    };
    let stderr = match child.stderr.take() {
        Some(pipe) => pipe,
        None => return Err(pipe_unavailable(&mut child, operation, "stderr")),
    };
    let stdout_reader = thread::spawn(move || read_pipe(stdout));
    let stderr_reader = thread::spawn(move || read_pipe(stderr));

    let status = loop {
        if context.cancellation.is_cancelled() {
            let cleanup = abort_command(&mut child, stdout_reader, stderr_reader, operation);
            return match cleanup {
                Ok(()) => Err(HostError::cancelled()),
                Err(error) => Err(HostError::internal(format!(
                    "{operation} cancellation cleanup failed: {}",
                    error.message
                ))),
            };
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(error) => {
                let cleanup = abort_command(&mut child, stdout_reader, stderr_reader, operation);
                return match cleanup {
                    Ok(()) => Err(HostError::internal(format!(
                        "{operation} wait failed: {error}"
                    ))),
                    Err(cleanup_error) => Err(HostError::internal(format!(
                        "{operation} wait failed: {error}; cleanup failed: {}",
                        cleanup_error.message
                    ))),
                };
            }
        }
    };
    let (stdout, stderr) = join_readers(stdout_reader, stderr_reader, operation)?;
    Ok(CommandOutput {
        status,
        stdout,
        stderr,
    })
}

fn read_pipe(mut pipe: impl Read) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    pipe.read_to_end(&mut data)?;
    Ok(data)
}

fn join_readers(
    stdout_reader: thread::JoinHandle<io::Result<Vec<u8>>>,
    stderr_reader: thread::JoinHandle<io::Result<Vec<u8>>>,
    operation: &str,
) -> HostResult<(Vec<u8>, Vec<u8>)> {
    let stdout = join_reader(stdout_reader, operation, "stdout")?;
    let stderr = join_reader(stderr_reader, operation, "stderr")?;
    Ok((stdout, stderr))
}

fn join_reader(
    reader: thread::JoinHandle<io::Result<Vec<u8>>>,
    operation: &str,
    stream: &str,
) -> HostResult<Vec<u8>> {
    match reader.join() {
        Ok(Ok(data)) => Ok(data),
        Ok(Err(error)) => Err(HostError::internal(format!(
            "{operation} failed reading {stream}: {error}"
        ))),
        Err(_) => Err(HostError::internal(format!(
            "{operation} {stream} reader panicked"
        ))),
    }
}

fn abort_command(
    child: &mut Child,
    stdout_reader: thread::JoinHandle<io::Result<Vec<u8>>>,
    stderr_reader: thread::JoinHandle<io::Result<Vec<u8>>>,
    operation: &str,
) -> HostResult<()> {
    let termination = terminate_child(child);
    let stdout = join_reader(stdout_reader, operation, "stdout");
    let stderr = join_reader(stderr_reader, operation, "stderr");
    termination?;
    stdout?;
    stderr?;
    Ok(())
}

fn terminate_child(child: &mut Child) -> HostResult<()> {
    let group_termination = terminate_process_group(child);
    let direct_termination = if !matches!(&group_termination, Ok(true)) {
        match child.kill() {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(HostError::internal(format!(
                    "failed to terminate child: {error}"
                )));
            }
        }
        Ok(())
    } else {
        Ok(())
    };
    let reap = child
        .wait()
        .map(|_| ())
        .map_err(|error| HostError::internal(format!("failed to reap child: {error}")));
    direct_termination?;
    reap?;
    group_termination.map(|_| ())
}

fn configure_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        // WHY: shell commands may fork descendants that inherit our output
        // pipes. Keep the operation in its own group so cancellation closes
        // the whole owned process tree before reader threads are joined.
        command.process_group(0);
    }
    #[cfg(not(unix))]
    drop(command);
}

fn terminate_process_group(child: &mut Child) -> HostResult<bool> {
    #[cfg(unix)]
    {
        if child
            .try_wait()
            .map_err(|error| HostError::internal(format!("failed to inspect child: {error}")))?
            .is_some()
        {
            return Ok(true);
        }
        let group = format!("-{}", child.id());
        let status = Command::new("/bin/kill")
            .arg("-KILL")
            .arg(group)
            .status()
            .map_err(|error| {
                HostError::internal(format!("failed to signal child group: {error}"))
            })?;
        Ok(status.success())
    }
    #[cfg(not(unix))]
    {
        drop(child);
        Ok(false)
    }
}

fn pipe_unavailable(child: &mut Child, operation: &str, stream: &str) -> HostError {
    let message = format!("{operation} did not provide a {stream} pipe");
    match terminate_child(child) {
        Ok(()) => HostError::internal(message),
        Err(error) => HostError::internal(format!("{message}; cleanup failed: {error}")),
    }
}

fn spawn_detached(opener: &Valor) -> HostResult<ProviderReply> {
    let args = string_list_arg(opener, 0, "args")?;
    let (program, program_args) = args.split_first().ok_or_else(|| {
        HostError::invalid_args("processus:dimitte requires a non-empty args list")
    })?;
    let child = Command::new(program)
        .args(program_args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| HostError::internal(format!("processus:dimitte failed: {error}")))?;
    Ok(ProviderReply::item(Valor::Numerus(child.id() as i64)))
}

fn read_env(opener: &Valor) -> HostResult<ProviderReply> {
    let name = string_arg(opener, 0, "nomen")?;
    match std::env::var(&name) {
        Ok(value) => Ok(ProviderReply::item(Valor::Textus(value))),
        Err(_) => Err(HostError::internal(format!(
            "processus:lege: environment variable `{name}` is not set"
        ))),
    }
}

fn write_env(opener: &Valor) -> HostResult<ProviderReply> {
    let name = string_arg(opener, 0, "nomen")?;
    let value = string_arg(opener, 1, "valor")?;
    #[allow(unused_unsafe)]
    unsafe {
        std::env::set_var(name, value);
    }
    Ok(ProviderReply::vacuum())
}

fn current_dir() -> HostResult<ProviderReply> {
    let path = std::env::current_dir()
        .map_err(|error| HostError::internal(format!("processus:sedes failed: {error}")))?;
    Ok(ProviderReply::item(Valor::Textus(
        path.to_string_lossy().into_owned(),
    )))
}

fn set_current_dir(opener: &Valor) -> HostResult<ProviderReply> {
    let path = string_arg(opener, 0, "via")?;
    std::env::set_current_dir(&path)
        .map_err(|error| HostError::internal(format!("processus:muta failed: {error}")))?;
    Ok(ProviderReply::vacuum())
}

fn exit_process(opener: &Valor) -> HostResult<ProviderReply> {
    let code = i64_arg(opener, 0, "code")?;
    std::process::exit(code.clamp(0, i64::from(u8::MAX)) as i32);
}

fn positional<'a>(value: &'a Valor, index: usize, name: &str) -> HostResult<&'a Valor> {
    match value {
        Valor::Lista(values) => values.get(index).ok_or_else(|| {
            HostError::invalid_args(format!("missing positional argument {index} ({name})"))
        }),
        value if index == 0 => Ok(value),
        _ => Err(HostError::invalid_args(format!(
            "missing positional argument {index} ({name})"
        ))),
    }
}

fn i64_arg(value: &Valor, index: usize, name: &str) -> HostResult<i64> {
    match positional(value, index, name)? {
        Valor::Numerus(number) => Ok(*number),
        _ => Err(HostError::invalid_args(format!(
            "{name} must be an integer"
        ))),
    }
}

fn string_arg(value: &Valor, index: usize, name: &str) -> HostResult<String> {
    match positional(value, index, name)? {
        Valor::Textus(text) | Valor::Instans(text) => Ok(text.clone()),
        _ => Err(HostError::invalid_args(format!("{name} must be a string"))),
    }
}

fn string_list_arg(value: &Valor, index: usize, name: &str) -> HostResult<Vec<String>> {
    let value = if index == 0 {
        match value {
            Valor::Lista(values) if values.iter().all(|item| matches!(item, Valor::Textus(_))) => {
                value
            }
            _ => positional(value, index, name)?,
        }
    } else {
        positional(value, index, name)?
    };
    match value {
        Valor::Lista(values) => values
            .iter()
            .map(|item| match item {
                Valor::Textus(text) | Valor::Instans(text) => Ok(text.clone()),
                _ => Err(HostError::invalid_args(format!(
                    "{name} must contain strings"
                ))),
            })
            .collect(),
        _ => Err(HostError::invalid_args(format!(
            "{name} must be a list of strings"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use host_kernel::ProviderContent;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn manifest_registers_all_process_routes() {
        let mut kernel = Kernel::new();
        register(&mut kernel).expect("register processus");
        assert_eq!(kernel.manifest().providers[0].calls.len(), 11);
    }

    #[test]
    fn capture_returns_structured_status_stdout_and_stderr() {
        let provider = Processus::new().expect("provider");
        let reply = provider
            .dispatch(
                &RequestFrame {
                    conversation_id: "capture".into(),
                    route: "processus:captura".into(),
                    opener: Valor::Lista(vec![
                        Valor::Textus("sh".into()),
                        Valor::Textus("-c".into()),
                        Valor::Textus("printf out; printf err >&2; exit 7".into()),
                    ]),
                    target: None,
                },
                &DispatchContext {
                    cancellation: host_kernel::CancellationProbe::new(|| false),
                },
            )
            .expect("capture");
        let [ProviderContent::Item(Valor::Tabula(fields))] = reply.contents.as_slice() else {
            panic!("capture must return one tabula item");
        };
        assert_eq!(fields.get("status"), Some(&Valor::Numerus(7)));
        assert_eq!(fields.get("stdout"), Some(&Valor::Textus("out".into())));
        assert_eq!(fields.get("stderr"), Some(&Valor::Textus("err".into())));
    }

    #[test]
    fn shell_route_returns_stdout_item() {
        let provider = Processus::new().expect("provider");
        let reply = provider
            .dispatch(
                &RequestFrame {
                    conversation_id: "shell".into(),
                    route: "processus:exsequi".into(),
                    opener: Valor::Textus("printf salve".into()),
                    target: None,
                },
                &DispatchContext {
                    cancellation: host_kernel::CancellationProbe::new(|| false),
                },
            )
            .expect("shell");
        assert!(
            matches!(reply.contents.as_slice(), [ProviderContent::Item(Valor::Textus(text))] if text == "salve")
        );
    }

    fn dispatch_until_cancelled(provider: &Processus, route: &str, opener: Valor) -> HostError {
        let cancelled = Arc::new(AtomicBool::new(false));
        let trigger = Arc::clone(&cancelled);
        let timer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(25));
            trigger.store(true, Ordering::SeqCst);
        });
        let result = provider.dispatch(
            &RequestFrame {
                conversation_id: route.into(),
                route: route.into(),
                opener,
                target: None,
            },
            &DispatchContext {
                cancellation: host_kernel::CancellationProbe::from_flag(cancelled),
            },
        );
        timer.join().expect("cancellation timer");
        result.expect_err("running process must be cancelled")
    }

    #[test]
    fn cancellation_terminates_shell_and_capture_children() {
        let provider = Processus::new().expect("provider");
        let started = std::time::Instant::now();
        let shell_error = dispatch_until_cancelled(
            &provider,
            "processus:exsequi",
            Valor::Textus("while :; do :; done".into()),
        );
        assert_eq!(shell_error.code, "E_CANCELLED");
        let capture_error = dispatch_until_cancelled(
            &provider,
            "processus:captura",
            Valor::Lista(vec![
                Valor::Textus("sh".into()),
                Valor::Textus("-c".into()),
                Valor::Textus("while :; do :; done".into()),
            ]),
        );
        assert_eq!(capture_error.code, "E_CANCELLED");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "cancelled child operations must not block indefinitely"
        );
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_terminates_shell_descendants() {
        let provider = Processus::new().expect("provider");
        let started = std::time::Instant::now();
        let error = dispatch_until_cancelled(
            &provider,
            "processus:exsequi",
            Valor::Textus("sleep 30".into()),
        );
        assert_eq!(error.code, "E_CANCELLED");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "cancellation must terminate shell descendants with the owned process"
        );
    }
}
