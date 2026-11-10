use std::sync::Arc;

use arrow::util::display::{ArrayFormatter, FormatOptions};
use parquet::arrow::arrow_reader::{
    ArrowReaderMetadata, ArrowReaderOptions, ParquetRecordBatchReaderBuilder,
};
use parquet::file::metadata::PageIndexPolicy;
use tempfile::NamedTempFile;

use super::{MAX_PREVIEW_ROWS, ParquetSchemaPreview, PreviewFileType, PreviewScope, TablePreview};

/// Bound decoded rows independently of the size of the downloaded Parquet file.
pub const PARQUET_PAGE_ROWS: usize = 256;

/// A downloaded file and reusable metadata. Dropping the last owner removes the file.
#[derive(Debug)]
pub struct ParquetFilePreview {
    file: NamedTempFile,
    metadata: ArrowReaderMetadata,
    schema: ParquetSchemaPreview,
}

#[derive(Debug)]
pub struct ParquetPage {
    pub offset: usize,
    pub table: Arc<TablePreview>,
}

impl ParquetFilePreview {
    /// Cache schema and page offsets without decoding the table.
    ///
    /// # Errors
    /// Returns an error if the cached file cannot be opened or its metadata is invalid.
    pub fn open(file: NamedTempFile) -> Result<Self, String> {
        let reader = file
            .reopen()
            .map_err(|e| format!("Failed to open preview: {e}"))?;
        let file_size = reader
            .metadata()
            .map_err(|e| format!("Failed to stat preview: {e}"))?
            .len();
        let options =
            ArrowReaderOptions::default().with_offset_index_policy(PageIndexPolicy::Optional);
        let metadata = ArrowReaderMetadata::load(&reader, options)
            .map_err(|e| format!("Failed to read Parquet metadata: {e}"))?;
        let parquet = metadata.metadata();
        let schema = ParquetSchemaPreview {
            fields: parquet
                .file_metadata()
                .schema()
                .get_fields()
                .iter()
                .map(|field| format!("{}: {}", field.name(), super::format_parquet_type(field)))
                .collect(),
            num_row_groups: parquet.num_row_groups(),
            num_rows: parquet.file_metadata().num_rows(),
            file_size: Some(file_size),
            created_by: parquet
                .file_metadata()
                .created_by()
                .map(ToString::to_string),
            note: None,
        };
        Ok(Self {
            file,
            metadata,
            schema,
        })
    }

    #[must_use]
    pub fn schema(&self) -> ParquetSchemaPreview {
        self.schema.clone()
    }

    #[must_use]
    pub fn row_count(&self) -> usize {
        usize::try_from(self.schema.num_rows).unwrap_or(0)
    }

    /// Decode a bounded page, skipping preceding row groups and data pages when possible.
    ///
    /// # Errors
    /// Returns an error if reading, decoding, or formatting the requested rows fails.
    pub fn read_page(&self, offset: usize, scope: PreviewScope) -> Result<ParquetPage, String> {
        // Reopen rather than clone a file descriptor: concurrent readers need independent cursors.
        let file = self
            .file
            .reopen()
            .map_err(|e| format!("Failed to open preview: {e}"))?;
        let limit = match scope {
            PreviewScope::Limited => MAX_PREVIEW_ROWS,
            PreviewScope::Full => PARQUET_PAGE_ROWS,
        };
        let reader =
            ParquetRecordBatchReaderBuilder::new_with_metadata(file, self.metadata.clone())
                .with_batch_size(limit)
                .with_offset(offset)
                .with_limit(limit)
                .build()
                .map_err(|e| format!("Failed to read Parquet rows: {e}"))?;
        let schema = self.metadata.schema();
        let mut rows = Vec::new();
        let options = FormatOptions::default();
        for batch in reader {
            let batch = batch.map_err(|e| format!("Failed to read Parquet batch: {e}"))?;
            // Reuse column formatters instead of building one for every cell.
            let formatters = batch
                .columns()
                .iter()
                .map(|column| {
                    ArrayFormatter::try_new(column.as_ref(), &options)
                        .map_err(|e| format!("Failed to format Parquet column: {e}"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            for row in 0..batch.num_rows() {
                rows.push(
                    batch
                        .columns()
                        .iter()
                        .zip(&formatters)
                        .map(|(column, formatter)| {
                            if column.is_null(row) {
                                "null".to_string()
                            } else {
                                formatter.value(row).to_string()
                            }
                        })
                        .collect(),
                );
            }
        }
        Ok(ParquetPage {
            offset,
            table: Arc::new(TablePreview {
                headers: schema
                    .fields()
                    .iter()
                    .map(|field| field.name().clone())
                    .collect(),
                column_types: Some(
                    schema
                        .fields()
                        .iter()
                        .map(|field| field.data_type().to_string())
                        .collect(),
                ),
                rows,
                total_rows: self.row_count(),
                truncated: scope == PreviewScope::Limited && self.row_count() > limit,
                file_type: PreviewFileType::Parquet,
            }),
        })
    }

    #[cfg(test)]
    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        self.file.path()
    }
}

/// Shared ownership keeps the cached download alive while a page is being read.
pub type SharedParquetFile = Arc<ParquetFilePreview>;

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::{array::Int32Array, record_batch::RecordBatch};
    use parquet::{arrow::ArrowWriter, file::properties::WriterProperties};

    #[test]
    fn pages_without_offset_indexes_cross_row_groups_and_preserve_nulls() {
        let file = NamedTempFile::new().unwrap();
        let batch = RecordBatch::try_from_iter([(
            "value",
            Arc::new(Int32Array::from_iter(
                (0..1500).map(|i| (i % 10 != 0).then_some(i)),
            )) as arrow::array::ArrayRef,
        )])
        .unwrap();
        let properties = WriterProperties::builder()
            .set_offset_index_disabled(true)
            .set_max_row_group_row_count(Some(400))
            .build();
        let mut writer =
            ArrowWriter::try_new(file.reopen().unwrap(), batch.schema(), Some(properties)).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();
        let preview = ParquetFilePreview::open(file).unwrap();
        assert!(preview.metadata.metadata().offset_index().is_none());
        for offset in [0, 350, 1490] {
            let page = preview.read_page(offset, PreviewScope::Full).unwrap();
            assert_eq!(page.offset, offset);
            assert_eq!(
                page.table.rows.len(),
                (1500 - offset).min(PARQUET_PAGE_ROWS)
            );
            for (index, row) in page.table.rows.iter().enumerate() {
                let value = offset + index;
                assert_eq!(
                    row[0],
                    if value % 10 == 0 {
                        "null".to_string()
                    } else {
                        value.to_string()
                    }
                );
            }
        }
    }
}
