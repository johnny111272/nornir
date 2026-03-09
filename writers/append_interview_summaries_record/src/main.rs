use schemas_embedded::SUMMARIES;
use write_core::{OutputFormat, OutputPath, WriteFrequency, WriterConfig};

fn main() {
    match write_core::run(&WriterConfig {
        name: "append_interview_summaries_record",
        schema: &SUMMARIES,
        schema_source_path: "/Users/johnny/.ai/spaces/bragi/schemas/summaries.schema.json",
        format: OutputFormat::Jsonl,
        frequency: WriteFrequency::Record,
        output: OutputPath::DirectoryPrefix {
            dir: "/Users/johnny/.ai/spaces/bragi/interview/interviews",
            suffix: ".summaries.jsonl",
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
