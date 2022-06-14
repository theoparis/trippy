use bpaf::{construct, positional, short, Info};
use miette::Result;
use std::path::Path;
use trippy_compiler_core::compile;
use trippy_compiler_core::interpret;
use trippy_compiler_core::{build, parse, TrippyError};

#[derive(Clone, Debug)]
struct Opts {
	output: Option<String>,
	path: String,
}

fn opts() -> Opts {
	let path = positional("path").from_str();
	let output = short('o')
		.long("output")
		.help("output file path to write a compiled binary to")
		.argument("OUTPUT")
		.optional();

	// combine parsers `speed` and `distance` parsers into a parser for Opts
	let parser = construct!(Opts { path, output });

	// define help message, attach it to parser, and run the results
	Info::default()
		.descr("Accept speed and distance, print them")
		.for_parser(parser)
		.run()
}

fn main() -> Result<()> {
	let opts = opts();

	let src_name = Path::new(&opts.path).file_name().unwrap().to_str().unwrap();
	let src =
		std::fs::read_to_string(&opts.path).map_err(TrippyError::IoError)?;

	let ast = parse(&src);

	let ctx = inkwell::context::Context::create();
	let module = build(&ctx, ast, src_name)?;

	if let Some(output_file) = opts.output {
		let obj = compile(&module)?;
		let obj_name = src_name.replace(".js", "").replace(".ts", "");
		let obj_path = format!("{}.o", obj_name);

		std::fs::write(obj_path.clone(), obj).map_err(TrippyError::IoError)?;

		std::process::Command::new("clang")
			.arg("-static")
			.arg("-o")
			.arg(output_file)
			.arg(obj_path)
			.status()
			.map_err(TrippyError::IoError)?;
	} else {
		interpret(&module)?;
	}

	Ok(())
}
