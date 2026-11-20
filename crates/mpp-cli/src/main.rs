use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use mpp_cli::{calc_line, check_report, run_file, run_lines, run_report_with};
use mpp_export::{export_path, load, render_page, ExportOptions, Format, PageSize};

#[derive(Parser)]
#[command(
    name = "mpp",
    version,
    about = "Math++: write maths like a paper, and run it"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a file and print the answer to each query
    Run {
        file: PathBuf,
        /// Show the working of `= ?!` queries under their answers
        #[arg(long)]
        steps: bool,
    },
    /// Check files for errors (exit code 1 when there are any)
    Check {
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Write a static HTML page with typeset maths, graphs and results
    Render {
        file: PathBuf,
        /// The page to write (default: the file's name with .html)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Export to PDF, LaTeX or HTML
    Export {
        file: PathBuf,
        /// pdf, tex or html
        #[arg(long, value_name = "FORMAT")]
        to: String,
        /// The file to write (default: the file's name with the new extension)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Leave the answers of queries out
        #[arg(long)]
        no_results: bool,
        /// Show the working of `= ?!` queries
        #[arg(long)]
        steps: bool,
        /// Show comments as notes
        #[arg(long)]
        comments: bool,
        /// A dark page (the default is light, for printing)
        #[arg(long)]
        dark: bool,
        /// PDF paper size
        #[arg(long, value_enum, default_value = "a4")]
        page: Paper,
        /// PDF margin in millimetres
        #[arg(long, default_value_t = 25.0)]
        margin: f32,
    },
    /// Quick calculator: mpp calc "\int_0^1 x^2 dx"
    Calc {
        #[arg(required = true, num_args = 1..)]
        expression: Vec<String>,
    },
    /// Open the Math++ editor
    Ide { file: Option<PathBuf> },
}

#[derive(Clone, Copy, ValueEnum)]
enum Paper {
    A4,
    Letter,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Run { file, steps } => run(&file, steps),
        Command::Check { files } => check(&files),
        Command::Render { file, output } => render(&file, output),
        Command::Export {
            file,
            to,
            output,
            no_results,
            steps,
            comments,
            dark,
            page,
            margin,
        } => {
            let opts = ExportOptions {
                include_results: !no_results,
                include_steps: steps,
                include_comments: comments,
                dark,
                page: match page {
                    Paper::A4 => PageSize::A4,
                    Paper::Letter => PageSize::Letter,
                },
                margin_mm: margin,
            };
            export(&file, &to, output, &opts)
        }
        Command::Calc { expression } => calc(&expression.join(" ")),
        Command::Ide { file } => ide(file.as_deref()),
    }
}

fn fail(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("error: {msg}");
    ExitCode::FAILURE
}

fn run(file: &Path, steps: bool) -> ExitCode {
    match run_file(file) {
        Ok(ev) => {
            print!("{}", run_report_with(&ev, steps));
            if run_lines(&ev).iter().any(|l| l.error) {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => fail(e),
    }
}

fn check(files: &[PathBuf]) -> ExitCode {
    let mut bad = false;
    for file in files {
        match run_file(file) {
            Ok(ev) => {
                let name = file.display().to_string();
                let report = check_report(&name, &ev);
                if report.is_empty() {
                    println!("{name}: ok");
                } else {
                    eprint!("{report}");
                }
                if !ev.errors().is_empty() {
                    bad = true;
                }
            }
            Err(e) => {
                eprintln!("error: {e}");
                bad = true;
            }
        }
    }
    if bad {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn render(file: &Path, output: Option<PathBuf>) -> ExitCode {
    let loaded = match load(file) {
        Ok(l) => l,
        Err(e) => return fail(e),
    };
    let out = output.unwrap_or_else(|| file.with_extension("html"));
    match std::fs::write(&out, render_page(&loaded)) {
        Ok(()) => {
            println!("wrote {}", out.display());
            ExitCode::SUCCESS
        }
        Err(e) => fail(format!("could not write {}: {e}", out.display())),
    }
}

fn export(file: &Path, to: &str, output: Option<PathBuf>, opts: &ExportOptions) -> ExitCode {
    let Some(format) = Format::parse(to) else {
        return fail(format!("unknown format `{to}`; use pdf, tex or html"));
    };
    let out = output.unwrap_or_else(|| file.with_extension(format.extension()));
    let exported = match export_path(file, format, opts) {
        Ok(e) => e,
        Err(e) => return fail(e),
    };
    if let Err(e) = std::fs::write(&out, &exported.main) {
        return fail(format!("could not write {}: {e}", out.display()));
    }
    let dir = out.parent().filter(|p| !p.as_os_str().is_empty());
    for (name, bytes) in &exported.files {
        let path = dir.map_or_else(|| PathBuf::from(name), |d| d.join(name));
        if let Err(e) = std::fs::write(&path, bytes) {
            return fail(format!("could not write {}: {e}", path.display()));
        }
    }
    println!("wrote {}", out.display());
    ExitCode::SUCCESS
}

fn calc(input: &str) -> ExitCode {
    match calc_line(input) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(e),
    }
}

/// Finds the editor next to `mpp` or on the PATH and starts it.
fn ide(file: Option<&Path>) -> ExitCode {
    let exe_name = if cfg!(windows) {
        "mathpp-ide.exe"
    } else {
        "mathpp-ide"
    };
    let beside = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(exe_name)))
        .filter(|p| p.exists());
    let program = beside.unwrap_or_else(|| PathBuf::from("mathpp-ide"));
    let mut cmd = std::process::Command::new(&program);
    if let Some(f) = file {
        cmd.arg(f);
    }
    match cmd.spawn() {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => fail(
            "the Math++ editor (mathpp-ide) is not built or not on the PATH; \
             build it with `cargo build -p mpp-ide` and try again",
        ),
        Err(e) => fail(format!("could not start the editor: {e}")),
    }
}
