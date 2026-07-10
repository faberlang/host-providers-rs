//! Public `consolum` provider.

use faber::Valor;
use host_kernel::{
    DispatchContext, HostError, HostResult, Kernel, Provider, ProviderRegistration, ProviderReply,
    RequestFrame,
};
use std::io::{self, IsTerminal, Read, Write};
use std::sync::Arc;

pub struct Consolum {
    registration: ProviderRegistration,
}

impl Consolum {
    pub fn new() -> HostResult<Self> {
        Ok(Self {
            registration: ProviderRegistration::new(host_kernel::parse_manifest(manifest_json())?),
        })
    }
}

pub fn register(kernel: &mut Kernel) -> HostResult<()> {
    kernel.register(Arc::new(Consolum::new()?))
}

pub fn manifest_json() -> &'static str {
    include_str!("manifest.json")
}

impl Provider for Consolum {
    fn registration(&self) -> &ProviderRegistration {
        &self.registration
    }

    fn dispatch(
        &self,
        request: &RequestFrame,
        _context: &DispatchContext,
    ) -> HostResult<ProviderReply> {
        match request.route.as_str() {
            "consolum:hauri" | "consolum:hauriet" => read_stdin(&request.opener),
            "consolum:lege" | "consolum:leget" => read_line(),
            "consolum:funde" => write_stdout_bytes(&request.opener),
            "consolum:scribe" | "consolum:scribet" => write_stdout_line(&request.opener),
            "consolum:dic" | "consolum:dicet" => write_stdout(&request.opener),
            "consolum:mone" | "consolum:monet" | "consolum:vide" | "consolum:videbit" => {
                write_stderr_line(&request.opener)
            }
            "consolum:audit" => Ok(ProviderReply::item(Valor::Bivalens(
                io::stdin().is_terminal(),
            ))),
            "consolum:loquitur" => Ok(ProviderReply::item(Valor::Bivalens(
                io::stdout().is_terminal(),
            ))),
            "consolum:admonet" => Ok(ProviderReply::item(Valor::Bivalens(
                io::stderr().is_terminal(),
            ))),
            other => Err(HostError::no_route(format!(
                "no built-in consolum syscall registered for {other}"
            ))),
        }
    }
}

fn read_stdin(opener: &Valor) -> HostResult<ProviderReply> {
    let magnitude = i64_arg(opener, 0, "magnitudo")?.max(0) as usize;
    let mut buffer = vec![0_u8; magnitude];
    let bytes_read = io::stdin()
        .lock()
        .read(&mut buffer)
        .map_err(|error| HostError::internal(format!("failed to read stdin: {error}")))?;
    buffer.truncate(bytes_read);
    Ok(ProviderReply::byte(buffer))
}

fn read_line() -> HostResult<ProviderReply> {
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|error| HostError::internal(format!("failed to read stdin line: {error}")))?;
    trim_line_ending(&mut line);
    Ok(ProviderReply::item(Valor::Textus(line)))
}

fn write_stdout_bytes(opener: &Valor) -> HostResult<ProviderReply> {
    let data = bytes_arg(opener, 0, "data")?;
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(&data)
        .and_then(|()| stdout.flush())
        .map_err(|error| HostError::internal(format!("failed to write stdout: {error}")))?;
    Ok(ProviderReply::vacuum())
}

fn write_stdout_line(opener: &Valor) -> HostResult<ProviderReply> {
    let message = string_arg(opener, 0, "msg")?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{message}")
        .and_then(|()| stdout.flush())
        .map_err(|error| HostError::internal(format!("failed to write stdout: {error}")))?;
    Ok(ProviderReply::vacuum())
}

fn write_stdout(opener: &Valor) -> HostResult<ProviderReply> {
    let message = string_arg(opener, 0, "msg")?;
    let mut stdout = io::stdout().lock();
    write!(stdout, "{message}")
        .and_then(|()| stdout.flush())
        .map_err(|error| HostError::internal(format!("failed to write stdout: {error}")))?;
    Ok(ProviderReply::vacuum())
}

fn write_stderr_line(opener: &Valor) -> HostResult<ProviderReply> {
    let message = string_arg(opener, 0, "msg")?;
    let mut stderr = io::stderr().lock();
    writeln!(stderr, "{message}")
        .and_then(|()| stderr.flush())
        .map_err(|error| HostError::internal(format!("failed to write stderr: {error}")))?;
    Ok(ProviderReply::vacuum())
}

fn trim_line_ending(line: &mut String) {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
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

fn bytes_arg(value: &Valor, index: usize, name: &str) -> HostResult<Vec<u8>> {
    match positional(value, index, name)? {
        Valor::Octeti(bytes) => Ok(bytes.clone()),
        Valor::Textus(text) => Ok(text.as_bytes().to_vec()),
        Valor::Lista(items) => items
            .iter()
            .map(|item| match item {
                Valor::Numerus(byte) if (0..=u8::MAX as i64).contains(byte) => Ok(*byte as u8),
                _ => Err(HostError::invalid_args(format!(
                    "{name} must contain bytes"
                ))),
            })
            .collect(),
        _ => Err(HostError::invalid_args(format!(
            "{name} must be a byte array or string"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use host_kernel::ProviderContent;

    #[test]
    fn manifest_omits_fundet_alias_and_registers_canonical_routes() {
        let mut kernel = Kernel::new();
        register(&mut kernel).expect("register consolum");
        let calls = &kernel.manifest().providers[0].calls;
        assert_eq!(calls.len(), 16);
        assert!(calls.iter().any(|call| call.route == "consolum:funde"));
        assert!(!calls.iter().any(|call| call.route == "consolum:fundet"));
    }

    #[test]
    fn terminal_predicate_returns_one_boolean_item() {
        let provider = Consolum::new().expect("provider");
        let reply = provider
            .dispatch(
                &RequestFrame {
                    conversation_id: "audit".into(),
                    route: "consolum:audit".into(),
                    opener: Valor::Nihil,
                    target: None,
                },
                &DispatchContext {
                    cancellation: host_kernel::CancellationProbe::new(|| false),
                },
            )
            .expect("audit");
        assert!(matches!(
            reply.contents.as_slice(),
            [ProviderContent::Item(Valor::Bivalens(_))]
        ));
    }

    #[test]
    fn byte_and_string_arguments_decode_from_ordered_openers() {
        assert_eq!(
            bytes_arg(&Valor::Octeti(vec![1, 2]), 0, "data").unwrap(),
            vec![1, 2]
        );
        assert_eq!(
            string_arg(&Valor::Lista(vec![Valor::Textus("ok".into())]), 0, "msg").unwrap(),
            "ok"
        );
        assert!(i64_arg(&Valor::Textus("bad".into()), 0, "n").is_err());
    }
}
