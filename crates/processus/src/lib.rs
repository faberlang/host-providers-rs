//! Public `processus` provider.

use faber::Valor;
use host_kernel::{
    DispatchContext, HostError, HostResult, Kernel, Provider, ProviderRegistration, ProviderReply,
    RequestFrame,
};
use std::collections::BTreeMap;
use std::process::{Command, Stdio};
use std::sync::Arc;

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
        _context: &DispatchContext,
    ) -> HostResult<ProviderReply> {
        match request.route.as_str() {
            "processus:exsequi" | "processus:exsequetur" => execute_shell(&request.opener),
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
            "processus:captura" => capture_process(&request.opener),
            other => Err(HostError::no_route(format!(
                "no built-in processus syscall registered for {other}"
            ))),
        }
    }
}

fn execute_shell(opener: &Valor) -> HostResult<ProviderReply> {
    let command = string_arg(opener, 0, "imperium")?;
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .map_err(|error| HostError::internal(format!("processus:exsequi failed: {error}")))?;
    Ok(ProviderReply::item(Valor::Textus(
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )))
}

fn capture_process(opener: &Valor) -> HostResult<ProviderReply> {
    let args = string_list_arg(opener, 0, "args")?;
    let (program, program_args) = args.split_first().ok_or_else(|| {
        HostError::invalid_args("processus:captura requires a non-empty args list")
    })?;
    let output = Command::new(program)
        .args(program_args)
        .output()
        .map_err(|error| HostError::internal(format!("processus:captura failed: {error}")))?;
    let mut fields = BTreeMap::new();
    fields.insert(
        "status".to_owned(),
        Valor::Numerus(i64::from(output.status.code().unwrap_or(-1))),
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
}
