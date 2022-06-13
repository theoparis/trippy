use miette::Result;
use std::path::Path;
use trippy_compiler::{build, parse, TrippyError};

fn main() -> Result<()> {
	let src_path = std::env::args().nth(1).expect("Expected file argument");
	let src_name = Path::new(&src_path).file_name().unwrap().to_str().unwrap();
	let src = std::fs::read_to_string(src_path.clone())
		.map_err(TrippyError::IoError)?;

	let ast = parse(&src);

	//dbg!(ast.clone());

	let obj = build(ast, src_name)?;
	let obj_name = src_name.replace(".js", "").replace(".ts", "");
	let obj_path = format!("{}.o", obj_name);

	std::fs::write(obj_path.clone(), obj).map_err(TrippyError::IoError)?;

	std::process::Command::new("clang")
		.arg("-static")
		.arg("-o")
		.arg("main")
		.arg(obj_path)
		.status()
		.map_err(TrippyError::IoError)?;

	Ok(())
}
