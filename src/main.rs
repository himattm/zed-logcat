use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    match zlc::real_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // A closed downstream (Zed terminal pane closed, `| head`, etc.) surfaces as
            // BrokenPipe — that is a normal end of a tail, not an error.
            if let Some(ioe) = e.downcast_ref::<io::Error>() {
                if ioe.kind() == io::ErrorKind::BrokenPipe {
                    return ExitCode::SUCCESS;
                }
            }
            eprintln!("zlc: {e:#}");
            ExitCode::FAILURE
        }
    }
}
