use std::io::{self, Write};
use std::process::ExitCode;

const HELP: &str = "akasha-otsuge — local, traceable cross-agent memory

Usage: akasha-otsuge [--help | --version]

Options:
  -h, --help       Print help
  -V, --version    Print build version

Development bootstrap: memory commands are tracked in the MVP issues.
";
const VERSION: &str = concat!("akasha-otsuge ", env!("CARGO_PKG_VERSION"), "\n");

#[derive(Debug)]
enum CliError {
    Usage,
    Output,
}

impl CliError {
    fn report(&self) -> ExitCode {
        let (code, message, exit) = match self {
            Self::Usage => ("INVALID_ARGUMENT", "Use --help for supported arguments", 2),
            Self::Output => ("IO_ERROR", "Could not write command output", 1),
        };
        // Only fixed messages reach this formatter; arguments may contain credentials.
        let _ = writeln!(
            io::stderr().lock(),
            "{{\"schema_version\":1,\"error\":{{\"code\":\"{code}\",\"message\":\"{message}\"}}}}"
        );
        ExitCode::from(exit)
    }
}

fn run() -> Result<(), CliError> {
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    if args.next().is_some() {
        return Err(CliError::Usage);
    }
    let output = match first.as_deref() {
        None => HELP,
        Some(arg) if matches!(arg.to_str(), Some("-h" | "--help" | "help")) => HELP,
        Some(arg) if matches!(arg.to_str(), Some("-V" | "--version" | "version")) => VERSION,
        Some(_) => return Err(CliError::Usage),
    };
    io::stdout()
        .lock()
        .write_all(output.as_bytes())
        .map_err(|_| CliError::Output)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => error.report(),
    }
}
