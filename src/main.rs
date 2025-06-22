use std::fs::File;
use crate::playground::{Compiled, Error, Playground};
use clap::{arg, command, value_parser, Command};
use eframe::egui;
use std::path::PathBuf;
use std::sync::Arc;
use colored::Colorize;
use futures::future::MaybeDone::Future;
use futures::task::SpawnExt;
use crate::icombs::readback::{TypedHandle, TypedReadback};
use crate::par::types::Type;
use crate::readback::{Element, Event};
use crate::spawn::TokioSpawn;

mod icombs;
mod language_server;
mod location;
mod par;
mod playground;
mod readback;
mod spawn;

fn main() {
    let matches = command!()
        .subcommand_required(true)
        .subcommand(
            Command::new("playground")
                .about("Start the Par playground")
                .arg(
                    arg!([file] "Open a Par file in the playground")
                        .value_parser(value_parser!(PathBuf)),
                ),
        )
        .subcommand(
            Command::new("run")
                .about("Run a Par file in the playground")
                .arg(arg!(<file> "The Par file to run").value_parser(value_parser!(PathBuf)))
                .arg(arg!([function] "The function to run").default_value("main")),
        )
        .subcommand(
            Command::new("lsp").about("Start the Par language server for editor integration"),
        )
        .get_matches();

    match matches.subcommand() {
        Some(("playground", args)) => {
            let file = args.get_one::<PathBuf>("file");
            run_playground(file.cloned());
        }
        Some(("run", args)) => {
            let file = args.get_one::<PathBuf>("file").unwrap().clone();
            let function = args.get_one::<String>("function").unwrap().clone();
            run_function(file, function);
        }
        Some(("lsp", _)) => run_language_server(),
        _ => unreachable!(),
    }
}

fn run_playground(file: Option<PathBuf>) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1000.0, 700.0]),
            ..Default::default()
        };

        par::parse::set_miette_hook();

        eframe::run_native(
            "⅋layground",
            options,
            Box::new(|cc| Ok(Playground::new(cc, file))),
        )
        .expect("egui crashed");
    });
}

fn run_function(file: PathBuf, function: String) {
    // This function is currently not implemented.
    // It is intended to run a specific function from a Par file in the playground.
    // The implementation would involve reading the file, compiling it, and executing the specified function.

    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        println!("Running function '{}' from file '{}'", function, file.display());
        let Ok(code) = File::open(file).and_then(|mut file| {
            use std::io::Read;
            let mut buf = String::new();
            file.read_to_string(&mut buf)?;
            Ok(buf)
        }) else {
            println!("{}", "Could not read file".bright_red());
            return;
        };

        // let mut interact: Option<playground::Interact> = None;

        let compiled = match stacker::grow(32 * 1024 * 1024, || Compiled::from_string(&code)) {
            Ok(compiled) => { compiled }
            Err(err) => {
                println!("Compilation failed: {:?}", err);
                return;
            }
        };

        let Ok(checked) = compiled.checked else {
            println!("Type check failed");
            return;
        };

        let program = checked.program;

        let Some((name, definition)) = program
            .definitions
            .iter()
            .find(|(name, definition)| name.primary == function)
            .clone()
        else {
            println!("{}: {}", "Function not found".bright_red(), function);
            return;
        };

        let Some(ic_compiled) = checked.ic_compiled else {
            println!("{}: {}", "IC compilation failed".bright_red(), function);
            return;
        };

        let ty = ic_compiled.get_type_of(name).unwrap();

        let Type::Break(_) = ty else {
            println!("{}: {}", "Function is not a Break function".bright_red(), function);
            return;
        };

        let mut net = ic_compiled.create_net();
        let child_net = ic_compiled.get_with_name(name).unwrap();
        let tree = net.inject_net(child_net).with_type(ty.clone());

        let (mut net_arc, reducer_future) = net.start_reducer(Arc::new(TokioSpawn));

        // let ctx = ui.ctx().clone();
        let spawner = Arc::new(TokioSpawn);
        let readback_future = spawner.spawn_with_handle(async move {
            let mut handle = TypedHandle::from_wrapper(program.type_defs.clone(), net_arc, tree);
            loop {
                match handle.readback().await {
                    TypedReadback::Break => {
                        println!("Break function executed successfully.");
                        break;
                    },
                    _ => {
                        panic!("Unexpected readback from a break function.");
                    }
                }
            }

        }).unwrap();

        readback_future.await;
        reducer_future.await;
    });
}

// todo: this does not work
fn run_playground_function(_file: PathBuf, _function: String) {
    /*let Ok(code) = File::open(file).and_then(|mut file| {
        use std::io::Read;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        Ok(buf)
    }) else {
        println!("{}", "Could not read file".bright_red());
        return;
    };

    let mut interact: Option<playground::Interact> = None;

    let Ok(compiled) = stacker::grow(32 * 1024 * 1024, || Compiled::from_string(&code)) else {
        println!("Compilation failed");
        return;
    };
    let compiled_code = code.into();
    let program = compiled.program.clone();
    let Some(definition) = program
        .definitions
        .iter()
        .find(|definition| match &definition.name {
            Internal::Original(name) => name.string == function,
            _ => false,
        })
        .cloned()
    else {
        println!("{}: {}", "Function not found".bright_red(), function);
        return;
    };

    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1000.0, 700.0]),
            ..Default::default()
        };

        eframe::run_simple_native(
            "⅋layground - Run",
            options,
            move |ctx, _frame| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let name = definition.name.to_string();
                    if ui.button(&name).clicked() {
                        if let Some(int) = interact.take() {
                            int.handle.lock().expect("lock failed").cancel();
                        }
                        interact = Some(playground::Interact {
                            code: Arc::clone(&compiled_code),
                            handle: Handle::start_expression(
                                Arc::new({
                                    let ctx = ui.ctx().clone();
                                    move || ctx.request_repaint()
                                }),
                                Context::new(
                                    Arc::new(TokioSpawn),
                                    Arc::new(
                                        program
                                            .definitions
                                            .iter()
                                            .map(|Definition { name, expression, .. }| (name.clone(), expression.clone()))
                                            .collect(),
                                    ),
                                ),
                                &definition.expression,
                            ),
                        });
                    }
                });
            }
            // 417, 339, 508
        )
            .expect("egui crashed");
    })*/
}

fn run_language_server() {
    language_server::language_server_main::main()
}
