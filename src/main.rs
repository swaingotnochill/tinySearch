use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::result::Result;
use std::str;
use tiny_http::{Header, Method, Request, Response, Server};
use xml::common::{Position, TextPosition};
use xml::reader::{EventReader, XmlEvent};

struct Lexer<'a> {
    content: &'a [char],
}

impl<'a> Lexer<'a> {
    fn new(content: &'a [char]) -> Self {
        Self { content }
    }
    fn trim_left(&mut self) {
        while self.content.len() > 0 && self.content[0].is_whitespace() {
            self.content = &self.content[1..];
        }
    }

    fn chop(&mut self, n: usize) -> &'a [char] {
        let token = &self.content[0..n];
        self.content = &self.content[n..];
        return &token;
    }

    fn chop_while<P>(&mut self, mut predicate: P) -> &'a [char]
    where
        P: FnMut(&char) -> bool,
    {
        let mut idx = 0;
        while idx < self.content.len() && predicate(&self.content[idx]) {
            idx += 1;
        }
        self.chop(idx)
    }

    fn next_token(&mut self) -> Option<&'a [char]> {
        // trim whitespaces from left.
        self.trim_left();
        if self.content.len() == 0 {
            return None;
        }

        if self.content[0].is_numeric() {
            return Some(self.chop_while(|idx| idx.is_numeric()));
        }

        if self.content[0].is_alphabetic() {
            return Some(self.chop_while(|idx| idx.is_alphabetic()));
        }
        return Some(self.chop(1));
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = &'a [char];

    fn next(&mut self) -> Option<Self::Item> {
        self.next_token()
    }
}

fn index_document(_doc_content: &str) -> HashMap<String, usize> {
    todo!("not implemented");
}

fn parse_entire_xml_file(file_path: &Path) -> Result<String, ()> {
    let file = File::open(file_path).map_err(|err| {
        eprintln!(
            "ERROR: could not open file {file_path} due to {err}",
            file_path = file_path.display()
        );
    })?;
    let er = EventReader::new(file);
    let mut content = String::new();
    for event in er.into_iter() {
        let event = event.map_err(|err| {
            let TextPosition { row, column } = err.position();
            let msg = err.msg();
            eprintln!(
                "{file_path}: {row}: {column}: ERROR: {msg}",
                file_path = file_path.display()
            );
        })?;
        if let XmlEvent::Characters(text) = event {
            content.push_str(&text);
            content.push_str(" ");
        }
    }
    Ok(content)
}

type TermFreq = HashMap<String, usize>;
type TermFreqIndex = HashMap<PathBuf, TermFreq>;

fn save_tf_index(tf_index: &TermFreqIndex, index_path: &str) -> Result<(), ()> {
    println!("Saving {index_path}...");
    let index_file = File::create(index_path).map_err(|err| {
        eprintln!("ERROR: could not create index file {index_path}: {err}");
    })?;
    serde_json::to_writer(index_file, &tf_index).map_err(|err| {
        eprintln!("ERROR: could not write to index file {index_path}: {err}");
    })?;
    Ok(())
}

fn tf_index_of_folder(dir_path: &Path, tf_index: &mut TermFreqIndex) -> Result<(), ()> {
    let dir = fs::read_dir(dir_path).map_err(|err| {
        eprintln!(
            "ERROR: could not open directory {dir_path} fox indexing. Read full error: {err}",
            dir_path = dir_path.display()
        );
    })?;
    'next_file: for file in dir {
        let file = file.map_err(|err| {
            eprintln!(
                "ERROR: could not open directory {dir_path} for indexing. Read full error: {err}",
                dir_path = dir_path.display()
            )
        })?;
        let file_path = file.path();

        let file_type = file.file_type().map_err(|err| {
            eprintln!(
                "ERROR: could not determine file type of file {file_path}. Read full error: {err}",
                file_path = file_path.display()
            )
        })?;

        if file_type.is_dir() {
            tf_index_of_folder(&file_path, tf_index)?;
            continue 'next_file;
        }

        // TODO: Work with symlinks.

        println!("Indexing {file_path:?}...");

        let content = match parse_entire_xml_file(&file_path) {
            Ok(content) => content.chars().collect::<Vec<_>>(),
            Err(()) => continue 'next_file,
        };

        let mut tf = TermFreq::new();

        for token in Lexer::new(&content) {
            let term = token
                .iter()
                .map(|x| x.to_ascii_uppercase())
                .collect::<String>();
            if let Some(freq) = tf.get_mut(&term) {
                *freq += 1;
            } else {
                tf.insert(term, 1);
            }
        }
        tf_index.insert(file_path, tf);
    }
    Ok(())
}

fn check_index(index_path: &str) -> Result<(), ()> {
    let index_file = File::open(index_path)
        .map_err(|err| eprintln!("ERROR: could not open index file {index_path}: {err}"))?;

    println!("Reading {index_path} index file...");

    let tf_index: TermFreqIndex = serde_json::from_reader(&index_file)
        .map_err(|err| eprintln!("ERROR: could not parse index file {index_path}: {err}"))?;

    println!(
        "{index_path} contains {count} files",
        count = tf_index.len()
    );

    Ok(())
}

fn usage(program: &str) {
    eprintln!("Usage: {program} [SUBCOMMAND] [OPTIONS]");
    eprintln!("Subcommands: ");
    eprintln!("  index <folder>   index the <folder> and save the index to index.json file");
    eprintln!("  search <index-file>   check how many documents are indexed in the file (searching is not implemented yet)");
    eprintln!("  serve [address]   start the server at the address");
}

fn serve_static_file(request: Request, file_path: &str, content_type: &str) -> Result<(), ()> {
    let header = Header::from_bytes("Content-Type", content_type).unwrap();
    let file = File::open(file_path).map_err(|err| {
        eprintln!("ERROR: could not open file {file_path}: {err}");
    })?;
    let response = Response::from_file(file).with_header(header);
    request
        .respond(response)
        .unwrap_or_else(|err| eprintln!("ERROR: could not serve a request: {err}"));
    Ok(())
}

fn serve_404(request: Request) -> Result<(), ()> {
    request
        .respond(Response::from_string("404").with_status_code(404))
        .map_err(|err| eprintln!("ERROR: could not serve request: {err}"))?;
    Ok(())
}

fn serve_request(mut request: Request) -> Result<(), ()> {
    println!(
        "INFO: received request! method: {:?}, url : {:?}",
        request.method(),
        request.url(),
    );
    match (request.method(), request.url()) {
        (Method::Post, "/api/search") => {
            let mut buf = Vec::new();
            request.as_reader().read_to_end(&mut buf);
            let body = str::from_utf8(&buf).map_err(|err| {
                eprintln!("ERROR: could not interpret body as UTF-8 string : {err}")
            })?;
            println!("Search: {body}");
            request
                .respond(Response::from_string("ok"))
                .map_err(|err| eprintln!("ERROR: {err}"));
        }
        (Method::Get, "/") | (Method::Get, "/index.html") => {
            let index_html_path = "src/index.html";
            serve_static_file(request, index_html_path, "text/html, charset=utf-8")?;
        }
        (Method::Get, "/index.js") => {
            let index_js_path = "src/index.js";
            serve_static_file(request, index_js_path, "text/javascript, charset=utf-8")?;
        }
        _ => serve_404(request)?,
    }
    Ok(())
}

fn entry() -> Result<(), ()> {
    let mut args = env::args();
    let program = args.next().expect("path to program is provided.");

    let sub_command = args.next().ok_or_else(|| {
        usage(&program);
        eprintln!("ERROR: no subcommand is provided")
    })?;

    match sub_command.as_str() {
        "index" => {
            let dir_path = args.next().ok_or_else(|| {
                usage(&program);
                eprintln!("ERROR: no directory path is provided")
            })?;

            let mut tf_index = TermFreqIndex::new();
            tf_index_of_folder(Path::new(&dir_path), &mut tf_index);
            save_tf_index(&tf_index, "index.json");
        }
        "search" => {
            let index_path = args.next().ok_or_else(|| {
                usage(&program);
                eprintln!("ERROR: no path to index is provided for {sub_command} subcommand")
            })?;
            check_index(&index_path);
        }
        "serve" => {
            let address = args.next().unwrap_or("127.0.0.1:8888".to_string());
            let server = Server::http(&address).map_err(|err| {
                eprintln!("ERROR: could not start HTTP server at {address} : {err}");
            })?;

            println!("INFO: server listening at http://{address}/");

            for request in server.incoming_requests() {
                serve_request(request);
            }
        }
        _ => {
            usage(&program);
            eprintln!("ERROR: unknown subcommand {sub_command}.");
            return Err(());
        }
    }

    Ok(())
}

fn main() -> ExitCode {
    match entry() {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => ExitCode::FAILURE,
    }
}
