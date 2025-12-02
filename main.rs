use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Read};

fn load_snippets() -> HashMap<String, String> {
    if let Ok(data) = fs::read_to_string("snippets.json") {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        HashMap::new()
    }
}

fn save_snippets(snippets: &HashMap<String, String>) {
    let data = serde_json::to_string_pretty(snippets).expect("serialization failed");
    fs::write("snippets.json", data).expect("write failed");
}

fn print_usage() {
    eprintln!("Usage: --name <name> (read snippet from stdin and save)");
    eprintln!(" --read <name> (print snippet)");
    eprintln!(" --delete <name> (delete snippet)");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut snippets = load_snippets();

    if args.len() >= 3 && args[1] == "--name" {
        let name = args[2].clone();
        let mut content = String::new();
        io::stdin()
            .read_to_string(&mut content)
            .expect("failed to read stdin");
        snippets.insert(name, content);
        save_snippets(&snippets);
        println!("Snippet saved.");
    } else if args.len() >= 3 && args[1] == "--read" {
        let name = &args[2];
        if let Some(content) = snippets.get(name) {
            print!("{}", content);
        } else {
            eprintln!("Snippet not found.");
        }
    } else if args.len() >= 3 && args[1] == "--delete" {
        let name = &args[2];
        if snippets.remove(name).is_some() {
            save_snippets(&snippets);
            println!("Snippet deleted.");
        } else {
            eprintln!("Snippet not found.");
        }
    } else {
        print_usage();
    }
}
