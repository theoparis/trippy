use bpaf::{construct, positional, short, Info};
use miette::Result;
use std::path::Path;
use trippy_compiler_core::{
	parse, prepare, Compiler, CompilerModule, TrippyError,
};

#[derive(Clone, Debug)]
struct Opts {
	output: Option<String>,
	target: Option<String>,
	path: String,
}

fn opts() -> Opts {
	let path = positional("path").from_str();
	let output = short('o')
		.long("output")
		.help("output file path to write a compiled binary to")
		.argument("OUTPUT")
		.optional();
	let target = short('t')
		.long("target")
		.help("The output target to compile to, eg. x86_64-unknown-linux-musl")
		.argument("TARGET")
		.optional();

	// combine parsers `speed` and `distance` parsers into a parser for Opts
	let parser = construct!(Opts {
		path,
		output,
		target
	});

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

	let mut jit = Compiler::new(opts.output.is_none(), opts.target.clone())?;
	let func_id = prepare(&mut jit, ast)?;

	if let Some(output_file) = opts.output {
		let obj_name = src_name.replace(".js", "").replace(".ts", "");
		let obj_path = format!("{}.o", obj_name);
		let obj = jit.get_module_deref();

		match obj {
			CompilerModule::Object(obj) => {
				let obj_output = obj.finish().emit().unwrap();
				std::fs::write(obj_path.clone(), obj_output)
					.map_err(TrippyError::IoError)?;

				let cc =
					std::env::var("CC").unwrap_or_else(|_| "clang".to_string());

				std::process::Command::new(cc)
					.arg("-static")
					.arg("-target")
					.arg(
						&opts
							.target
							.unwrap_or_else(|| "x86_64-linux-musl".to_string()),
					)
					.arg("-o")
					.arg(output_file)
					.arg(obj_path)
					.status()
					.map_err(TrippyError::IoError)?;
			}
			_ => panic!("Unsupported module type for compilation output"),
		}
	} else {
		let obj = jit.get_module_deref();
		match obj {
			CompilerModule::Jit(mut jit) => {
				jit.finalize_definitions();

				let func = jit.get_finalized_function(func_id);
				unsafe {
					let func = std::mem::transmute::<_, fn() -> i64>(func);
					func();
				}
			}
			_ => panic!("Unsupported module type for interpretation"),
		}
	}

	Ok(())
}
