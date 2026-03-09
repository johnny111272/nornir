use schemas_embedded::RAW_JSONL_RECORD;
use write_core::{OutputFormat, OutputPath, WriteFrequency, WriterConfig};

fn main() {
    match write_core::run(&WriterConfig {
        name: "append_raw_jsonl",
        schema: &RAW_JSONL_RECORD,
        schema_source_path: "/Users/johnny/.ai/smidja/nornir/schemas/tools/raw-jsonl.schema.json",
        format: OutputFormat::Jsonl,
        frequency: WriteFrequency::Record,
        output: OutputPath::DirectoryName {
            dir: "/Users/johnny/.ai/traffic",
            ext: "jsonl",
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
