/*
 * Created on Wed Jul 01 2020
 *
 * This file is a part of Skytable
 * Skytable (formerly known as TerrabaseDB or Skybase) is a free and open-source
 * NoSQL database written by Sayan Nandan ("the Author") with the
 * vision to provide flexibility in data modelling without compromising
 * on performance, queryability or scalability.
 *
 * Copyright (c) 2020, Sayan Nandan <ohsayan@outlook.com>
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program. If not, see <https://www.gnu.org/licenses/>.
 *
*/

use crate::runner::Runner;
use clap::load_yaml;
use clap::App;
use crossterm::terminal::{Clear, ClearType};
use crossterm::{cursor, execute};
use libsky::URL;
use libsky::VERSION;
use readline::config::Configurer;
use readline::{error::ReadlineError, Editor};
use rustyline as readline;
use std::io::stdout;
use std::process;
const ADDR: &str = "127.0.0.1";
const SKYSH_BLANK: &str = "     > ";
const SKYSH_PROMPT: &str = "skysh> ";
const SKYSH_HISTORY_FILE: &str = ".sky_history";

const HELP_TEXT: &str = r#"
███████ ██   ██ ██    ██ ████████  █████  ██████  ██      ███████
██      ██  ██   ██  ██     ██    ██   ██ ██   ██ ██      ██
███████ █████     ████      ██    ███████ ██████  ██      █████
     ██ ██  ██     ██       ██    ██   ██ ██   ██ ██      ██
███████ ██   ██    ██       ██    ██   ██ ██████  ███████ ███████

Welcome to Skytable's interactive shell (REPL) environment. Using the Skytable
shell, you can create, read, update or delete data on your remote Skytable
instance. When you connect to your database instance, you'll be connected to
the `default` table in the `default` keyspace. This table has binary keys and
binary values as the default data type. Here's a brief guide on doing some
everyday tasks:

(1) Running actions
================================================================================
An action is like a shell command: it starts with a name and contains arguments!
To run actions, simply type them out, like "set x 100" or "inspect table mytbl"
and hit enter.

(2) Running shell commands
================================================================================
Shell commands are those which are provided by `skysh` and are not supported by
the server. These enable you to do convenient things like:
- "exit": exits the shell
- "clear": clears the terminal screen

Apart from these, you can use the following shell commands:
- "!help": Brings up this help menu
- "?<command name>": Describes what the built-in shell command is for

With Skytable in your hands, the sky is the only limit on what you can create!"#;

/// This creates a REPL on the command line and also parses command-line arguments
///
/// Anything that is entered following a return, is parsed into a query and is
/// written to the socket (which is either `localhost:2003` or it is determined by
/// command line parameters)
pub async fn start_repl() {
    let cfg_layout = load_yaml!("./cli.yml");
    let matches = App::from_yaml(cfg_layout).get_matches();
    let host = libsky::option_unwrap_or!(matches.value_of("host"), ADDR);
    let port = match matches.value_of("port") {
        Some(p) => match p.parse::<u16>() {
            Ok(p) => p,
            Err(_) => fatal!("Invalid port"),
        },
        None => 2003,
    };
    let mut editor = Editor::<()>::new();
    editor.set_auto_add_history(true);
    editor.set_history_ignore_dups(true);
    editor.bind_sequence(
        rustyline::KeyEvent(
            rustyline::KeyCode::BracketedPasteStart,
            rustyline::Modifiers::NONE,
        ),
        rustyline::Cmd::Noop,
    );
    let con = match matches.value_of("cert") {
        Some(cert) => Runner::new_secure(host, port, cert).await,
        None => Runner::new_insecure(host, port).await,
    };
    let mut runner = match con {
        Ok(c) => c,
        Err(e) => fatal!("Failed to connect to server with error: {}", e),
    };
    if let Some(eval_expr) = matches.value_of("eval") {
        if !eval_expr.is_empty() {
            runner.run_query(eval_expr).await;
        }
        process::exit(0x00);
    }
    println!("Skytable v{} | {}", VERSION, URL);
    match editor.load_history(SKYSH_HISTORY_FILE) {
        Ok(_) => {}
        Err(e) => match e {
            rustyline::error::ReadlineError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => {
                println!("{}", HELP_TEXT)
            }
            _ => fatal!("Failed to read history file with error: {}", e),
        },
    }
    loop {
        match editor.readline(SKYSH_PROMPT) {
            Ok(mut line) => match line.to_lowercase().as_str() {
                "exit" => break,
                "clear" => {
                    let mut stdout = stdout();
                    execute!(stdout, Clear(ClearType::All)).expect("Failed to clear screen");
                    execute!(stdout, cursor::MoveTo(0, 0))
                        .expect("Failed to move cursor to origin");
                    drop(stdout); // aggressively drop stdout
                    continue;
                }
                "help" => {
                    println!("To get help, run `!help`");
                    continue;
                }
                _ => {
                    if line.is_empty() {
                        continue;
                    }
                    match line.as_bytes()[0] {
                        b'#' => continue,
                        b'!' => {
                            // handle a shell command
                            match &line.as_bytes()[1..] {
                                b"" => eskysh!("Bad shell command"),
                                b"help" => println!("{}", HELP_TEXT),
                                _ => eskysh!("Unknown shell command"),
                            }
                            continue;
                        }
                        b'?' => {
                            // handle explanation for a shell command
                            match &line.as_bytes()[1..] {
                                b"" => eskysh!("Bad shell command"),
                                b"help" => println!("`!help` shows the help menu"),
                                b"exit" => println!("`exit` ends the shell session"),
                                b"clear" => println!("`clear` clears the terminal screen"),
                                _ => eskysh!("Unknown shell command"),
                            }
                            continue;
                        }
                        _ => {}
                    }
                    while line.len() >= 2 && line[line.len() - 2..].as_bytes().eq(br#" \"#) {
                        // continuation on next line
                        let cl = readln!(editor);
                        line.drain(line.len() - 2..);
                        line.extend(cl.chars());
                    }
                    runner.run_query(&line).await
                }
            },
            Err(ReadlineError::Interrupted) => break,
            Err(err) => fatal!("ERROR: Failed to read line with error: {}", err),
        }
    }
    editor
        .save_history(SKYSH_HISTORY_FILE)
        .map_err(|e| {
            fatal!("ERROR: Failed to save history with error: '{}'", e);
        })
        .unwrap();
}
