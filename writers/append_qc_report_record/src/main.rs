use schemas_embedded::QC_REPORT;
use write_core::{OutputFormat, OutputPath, WriteFrequency, WriterConfig};

fn main() {
    write_core::run(&WriterConfig {
        name: "append_qc_report_record",
        schema: &QC_REPORT,
        format: OutputFormat::Jsonl,
        frequency: WriteFrequency::Record,
        output: OutputPath::FixedFile(
            "/Users/johnny/.ai/spaces/bragi/truth/qc_semantic_report.jsonl",
        ),
        batch_size: None,
    });
}
