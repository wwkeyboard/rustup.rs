
#[macro_use]
extern crate clap;
extern crate rand;
extern crate regex;
extern crate hyper;
extern crate multirust;

use clap::{App, ArgMatches};
use std::env;
use std::path::{Path, PathBuf};
use std::io::BufRead;
use std::process::Command;
use std::ffi::OsString;
use multirust::*;

fn set_globals(m: Option<&ArgMatches>) -> Result<Cfg> {
	// Base config
	let verbose = m.map(|m| m.is_present("verbose")).unwrap_or(false);
	Cfg::from_env(NotifyHandler::from(move |n: Notification| {
		if verbose || !n.is_verbose() {
			println!("{}", n);
		}
	}))
		
}

fn main() {
	if let Err(e) = try_main() {
		println!("error: {}", e);
		std::process::exit(1);
	}
}

fn try_main() -> Result<()> {
	let mut arg_iter = env::args_os();
	let arg0 = PathBuf::from(arg_iter.next().unwrap());
	let arg0_stem = arg0.file_stem().expect("invalid multirust invocation")
		.to_str().expect("don't know how to proxy that binary");
	
	match arg0_stem {
		"multirust" | "multirust-rs" => {
			let arg1 = arg_iter.next();
			if let Some("run") = arg1.as_ref().and_then(|s| s.to_str()) {
				let arg2 = arg_iter.next().expect("expected binary name");
				let stem = arg2.to_str().expect("don't know how to proxy that binary");
				if !stem.starts_with("-") {
					run_proxy(stem, arg_iter)
				} else {
					run_multirust()
				}
			} else {
				run_multirust()
			}
		},
		"rustc" | "rustdoc" | "cargo" | "rust-lldb" | "rust-gdb" => {
			run_proxy(arg0_stem, arg_iter)
		},
		other => {
			Err(Error::Custom { id: "no-proxy".to_owned(), desc: format!("don't know how to proxy that binary: {}", other) })
		},
	}
}

fn current_dir() -> Result<PathBuf> {
	env::current_dir().map_err(|_| Error::LocatingWorkingDir)
}

fn run_proxy<I: Iterator<Item=OsString>>(binary: &str, arg_iter: I) -> Result<()> {
	let cfg = try!(set_globals(None));
	
	let mut command = try!(cfg.create_command_for_dir(&try!(current_dir()), binary));
	
	for arg in arg_iter {
		command.arg(arg);
	}
	let result = command.status()
		.ok().expect(&format!("failed to run `{}`", binary));
			
	// Ensure correct exit code is returned
	std::process::exit(result.code().unwrap_or(1));
}

fn run_multirust() -> Result<()> {
	let yaml = load_yaml!("cli.yml");
	let app_matches = App::from_yaml(yaml).get_matches();
	
	let cfg = try!(set_globals(Some(&app_matches)));
	
	match app_matches.subcommand_name() {
		Some("upgrade-data")|Some("delete-data") => {}, // Don't need consistent metadata
		Some(_) => { try!(cfg.check_metadata_version()); },
		_ => {},
	}
	
	match app_matches.subcommand() {
		("update", Some(m)) => update(&cfg, m),
		("default", Some(m)) => default_(&cfg, m),
		("override", Some(m)) => override_(&cfg, m),
		("show-default", Some(_)) => show_default(&cfg),
		("show-override", Some(_)) => show_override(&cfg),
		("list-overrides", Some(_)) => list_overrides(&cfg),
		("list-toolchains", Some(_)) => list_toolchains(&cfg),
		("remove-override", Some(m)) => remove_override(&cfg, m),
		("remove-toolchain", Some(m)) => remove_toolchain_args(&cfg, m),
		("upgrade-data", Some(_)) => cfg.upgrade_data().map(|_|()),
		("delete-data", Some(m)) => delete_data(&cfg, m),
		("which", Some(m)) => which(&cfg, m),
		("ctl", Some(m)) => ctl(&cfg, m),
		("doc", Some(m)) => doc(&cfg, m),
		_ => Ok(()),
	}
}

fn get_toolchain<'a>(cfg: &'a Cfg, m: &ArgMatches, create_parent: bool) -> Result<Toolchain<'a>> {
	cfg.get_toolchain(m.value_of("toolchain").unwrap(), create_parent)
}

fn remove_toolchain_args(cfg: &Cfg, m: &ArgMatches) -> Result<()> {
	try!(get_toolchain(cfg, m, false)).remove()
}

fn default_(cfg: &Cfg, m: &ArgMatches) -> Result<()> {
	let toolchain = try!(get_toolchain(cfg, m, false));
	if !try!(common_install_args(&toolchain, m)) {
		try!(toolchain.install_from_dist_if_not_installed());
	}
	
	toolchain.make_default()
}

fn override_(cfg: &Cfg, m: &ArgMatches) -> Result<()> {
	let toolchain = try!(get_toolchain(cfg, m, false));
	if !try!(common_install_args(&toolchain, m)) {
		try!(toolchain.install_from_dist_if_not_installed());
	}
	
	toolchain.make_override(&try!(current_dir()))
}

fn common_install_args(toolchain: &Toolchain, m: &ArgMatches) -> Result<bool> {
	
	if let Some(installers) = m.values_of("installer") {
		let is: Vec<_> = installers.iter().map(|i| i.as_ref()).collect();
		try!(toolchain.install_from_installers(&*is));
	} else if let Some(path) = m.value_of("copy-local") {
		try!(toolchain.install_from_dir(Path::new(path), false));
	} else if let Some(path) = m.value_of("link-local") {
		try!(toolchain.install_from_dir(Path::new(path), true));
	} else {
		return Ok(false);
	}
	Ok(true)
}

fn doc_url(m: &ArgMatches) -> &'static str {
	if m.is_present("all") {
		"index.html"
	} else {
		"std/index.html"
	}
}

fn doc(cfg: &Cfg, m: &ArgMatches) -> Result<()> {
	cfg.open_docs_for_dir(&try!(current_dir()), doc_url(m))
}

fn ctl_home(cfg: &Cfg) -> Result<()> {
	println!("{}", cfg.multirust_dir.display());
	Ok(())
}

fn ctl_overide_toolchain(cfg: &Cfg) -> Result<()> {
	let (toolchain, _) = try!(cfg.toolchain_for_dir(&try!(current_dir())));
	
	println!("{}", toolchain.name());
	Ok(())
}

fn ctl_default_toolchain(cfg: &Cfg) -> Result<()> {
	let toolchain = try!(try!(cfg.find_default()).ok_or(Error::NoDefaultToolchain));
	
	println!("{}", toolchain.name());
	Ok(())
}

fn ctl_toolchain_sysroot(cfg: &Cfg, m: &ArgMatches) -> Result<()> {
	let toolchain = try!(get_toolchain(cfg, m, false));
	
	let toolchain_dir = toolchain.prefix().path();
	println!("{}", toolchain_dir.display());
	Ok(())
}

fn ctl(cfg: &Cfg, m: &ArgMatches) -> Result<()> {
	match m.subcommand() {
		("home", Some(_)) => ctl_home(cfg),
		("override-toolchain", Some(_)) => ctl_overide_toolchain(cfg),
		("default-toolchain", Some(_)) => ctl_default_toolchain(cfg),
		("toolchain-sysroot", Some(m)) => ctl_toolchain_sysroot(cfg, m),
		_ => Ok(()),
	}
}

fn which(cfg: &Cfg, m: &ArgMatches) -> Result<()> {
	let binary = m.value_of("binary").unwrap();
	
	let binary_path = try!(cfg.which_binary(&try!(current_dir()), binary))
		.expect("binary not found");
	
	try!(utils::assert_is_file(&binary_path));

	println!("{}", binary_path.display());
	Ok(())
}

fn read_line() -> String {
	let stdin = std::io::stdin();
	let stdin = stdin.lock();
	let mut lines = stdin.lines();
	lines.next().unwrap().unwrap()
}

fn delete_data(cfg: &Cfg, m: &ArgMatches) -> Result<()> {
	if !m.is_present("no-prompt") {
		print!("This will delete all toolchains, overrides, aliases, and other multirust data associated with this user. Continue? (y/n) ");
		let input = read_line();
		
		match &*input {
			"y"|"Y" => {},
			_ => {
				println!("aborting");
				return Ok(());
			}
		}
	}
	
	cfg.delete_data()
}

fn remove_override(cfg: &Cfg, m: &ArgMatches) -> Result<()> {
	if let Some(path) = m.value_of("override") {
		cfg.override_db.remove(path.as_ref(), &cfg.temp_cfg, &cfg.notify_handler)
	} else {
		cfg.override_db.remove(&try!(current_dir()), &cfg.temp_cfg, &cfg.notify_handler)
	}.map(|_|())
}

fn show_tool_versions(toolchain: &Toolchain) -> Result<()> {
	println!("");

	if toolchain.exists() {
		let rustc_path = toolchain.prefix().binary_file("rustc");
		let cargo_path = toolchain.prefix().binary_file("cargo");

		try!(toolchain.prefix().with_ldpath(|| {
			if utils::is_file(&rustc_path) {
				Command::new(&rustc_path)
					.arg("--version")
					.status()
					.ok().expect("failed to run rustc");
			} else {
				println!("(no rustc command in toolchain?)");
			}
			if utils::is_file(&cargo_path) {
				Command::new(&cargo_path)
					.arg("--version")
					.status()
					.ok().expect("failed to run cargo");
			} else {
				println!("(no cargo command in toolchain?)");
			}
			Ok(())
		}));
	} else {
		println!("(toolchain not installed)");
	}
	println!("");
	Ok(())
}

fn show_default(cfg: &Cfg) -> Result<()> {
	if let Some(toolchain) = try!(cfg.find_default()) {
		println!("default toolchain: {}", toolchain.name());
		println!("default location: {}", toolchain.prefix().path().display());
		
		show_tool_versions(&toolchain)
	} else {
		println!("no default toolchain configured. run `multirust helpdefault`");
		Ok(())
	}
}

fn show_override(cfg: &Cfg) -> Result<()> {
	if let Some((toolchain, reason)) = try!(cfg.find_override(&try!(current_dir()))) {
		println!("override toolchain: {}", toolchain.name());
		println!("override location: {}", toolchain.prefix().path().display());
		println!("override reason: {}", reason);
		
		show_tool_versions(&toolchain)
	} else {
		println!("no override");
		show_default(cfg)
	}
}

fn list_overrides(cfg: &Cfg) -> Result<()> {
	let mut overrides = try!(cfg.override_db.list());
		
	overrides.sort();
	
	if overrides.is_empty() {
		println!("no overrides");
	} else {
		for o in overrides {
			println!("{}", o);
		}
	}
	Ok(())
}

fn list_toolchains(cfg: &Cfg) -> Result<()> {
	let mut toolchains = try!(cfg.list_toolchains());
		
	toolchains.sort();
	
	if toolchains.is_empty() {
		println!("no installed toolchains");
	} else {
		for toolchain in toolchains {
			println!("{}", &toolchain);
		}
	}
	Ok(())
}

fn update_all_channels(cfg: &Cfg) -> Result<()> {
	let result = cfg.update_all_channels();
	
	if result[0].is_ok() {
		println!("'stable' update succeeded");
	} else {
		println!("'stable' update FAILED");
	}
	if result[1].is_ok() {
		println!("'beta' update succeeded");
	} else {
		println!("'beta' update FAILED");
	}
	if result[2].is_ok() {
		println!("'nightly' update succeeded");
	} else {
		println!("'nightly' update FAILED");
	}
	
	println!("stable revision:");
	try!(show_tool_versions(&try!(cfg.get_toolchain("stable", false))));
	println!("beta revision:");
	try!(show_tool_versions(&try!(cfg.get_toolchain("beta", false))));
	println!("nightly revision:");
	try!(show_tool_versions(&try!(cfg.get_toolchain("nightly", false))));
	Ok(())
}

fn update(cfg: &Cfg, m: &ArgMatches) -> Result<()> {
	if let Some(name) = m.value_of("toolchain") {
		let toolchain = try!(cfg.get_toolchain(name, true));
		if !try!(common_install_args(&toolchain, m)) {
			try!(toolchain.install_from_dist())
		}
	} else {
		try!(update_all_channels(cfg))
	}
	Ok(())
}