use schemas_embedded::GLOSSARY;
use write_core::{OutputFormat, OutputPath, WriteFrequency, WriterConfig};

fn main() {
    write_core::run(&WriterConfig {
        name: "write_glossary_file",
        schema: &GLOSSARY,
        format: OutputFormat::Json,
        frequency: WriteFrequency::Record,
        output: OutputPath::DirectoryName {
            dir: "/Users/johnny/.ai/spaces/bragi/truth/quarantine",
            ext: "json",
        },
        batch_size: None,
    });
}
