use schemas_embedded::GLOSSARY;
use write_core::{OutputFormat, OutputPath, WriteFrequency, WriterConfig};

fn main() {
    match write_core::run(&WriterConfig {
        name: "write_truth_glossary_record",
        schema: &GLOSSARY,
        schema_source_path: "/Users/johnny/.ai/spaces/bragi/schemas/glossary.schema.json",
        format: OutputFormat::Json,
        frequency: WriteFrequency::Record,
        output: OutputPath::DirectoryName {
            dir: "/Users/johnny/.ai/spaces/bragi/truth/quarantine",
            ext: "json",
        },
        batch_size: None,
    }) {
        Ok(msg) => println!("{msg}"),
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
    }
}
