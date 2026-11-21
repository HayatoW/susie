use std::env;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    if engine::is_cef_subprocess() {
        return match engine::run_cef_subprocess() {
            Ok(exit_code) => std::process::exit(exit_code),
            Err(err) => {
                eprintln!("{err}");
                ExitCode::from(1)
            }
        };
    }

    let mut fixture = None;
    let mut output = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--fixture" => fixture = args.next(),
            "--output" => output = args.next(),
            "--help" => {
                eprintln!("usage: render-first-page --fixture FILE --output FILE");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("不明な引数です。{other}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(fixture), Some(output)) = (fixture, output) else {
        eprintln!("usage: render-first-page --fixture FILE --output FILE");
        return ExitCode::from(2);
    };

    match engine::render_fixture(Path::new(&fixture)) {
        Ok(engine::Rendered::Subprocess { exit_code }) => std::process::exit(exit_code),
        Ok(engine::Rendered::Frame(frame)) => match frame {
            engine::Frame::Cpu(pixels) => {
                if let Err(err) = engine::write_png(&pixels, Path::new(&output)) {
                    eprintln!("{err}");
                    return ExitCode::from(1);
                }
                println!("frame {}x{}", pixels.width(), pixels.height());
                ExitCode::SUCCESS
            }
        },
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}
