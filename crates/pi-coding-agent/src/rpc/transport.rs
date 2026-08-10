//! Strict, bounded LF-delimited JSON transport.

use std::io;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_RECORD_SIZE: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("invalid UTF-8")]
    Utf8,
    #[error("record exceeds {MAX_RECORD_SIZE} bytes")]
    Oversized,
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

/// Reads one record. Empty lines are ignored. Final nonempty EOF record accepted.
pub async fn read_record<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<Option<String>, TransportError> {
    loop {
        let mut bytes = Vec::new();
        loop {
            let available = reader.fill_buf().await?;
            if available.is_empty() {
                if bytes.is_empty() {
                    return Ok(None);
                }
                break;
            }
            let newline = available.iter().position(|&byte| byte == b'\n');
            let take = newline.map_or(available.len(), |position| position + 1);
            if bytes.len().saturating_add(take) > MAX_RECORD_SIZE + 2 {
                reader.consume(take);
                if newline.is_none() {
                    loop {
                        let chunk = reader.fill_buf().await?;
                        if chunk.is_empty() {
                            break;
                        }
                        let length = chunk.len();
                        let end = chunk.iter().position(|&byte| byte == b'\n').map(|p| p + 1);
                        reader.consume(end.unwrap_or(length));
                        if end.is_some() {
                            break;
                        }
                    }
                }
                return Err(TransportError::Oversized);
            }
            bytes.extend_from_slice(&available[..take]);
            reader.consume(take);
            if newline.is_some() {
                break;
            }
        }
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
        }
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        if bytes.len() > MAX_RECORD_SIZE {
            return Err(TransportError::Oversized);
        }
        if bytes.is_empty() {
            continue;
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| TransportError::Utf8)?;
        return Ok(Some(text.to_owned()));
    }
}

pub async fn write_record<W: AsyncWrite + Unpin, T: serde::Serialize>(
    writer: &mut W,
    value: &T,
) -> io::Result<()> {
    let mut record = serde_json::to_vec(value).map_err(io::Error::other)?;
    record.push(b'\n');
    writer.write_all(&record).await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn framing_crlf_and_final_record() {
        let mut reader = BufReader::new("{\"x\":\"  \"}\r\n\r\nnext".as_bytes());
        assert_eq!(
            read_record(&mut reader).await.unwrap().unwrap(),
            "{\"x\":\"  \"}"
        );
        assert_eq!(read_record(&mut reader).await.unwrap().unwrap(), "next");
        assert!(read_record(&mut reader).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn boundary_lf_crlf_eof() {
        // LF max and max+1
        let input_lf_ok = format!("{}\nvalid", "a".repeat(MAX_RECORD_SIZE));
        let mut r = BufReader::new(input_lf_ok.as_bytes());
        assert_eq!(
            read_record(&mut r).await.unwrap().unwrap().len(),
            MAX_RECORD_SIZE
        );
        assert_eq!(read_record(&mut r).await.unwrap().unwrap(), "valid");

        let input_lf_err = format!("{}\nvalid", "a".repeat(MAX_RECORD_SIZE + 1));
        let mut r = BufReader::new(input_lf_err.as_bytes());
        assert!(matches!(
            read_record(&mut r).await,
            Err(TransportError::Oversized)
        ));
        assert_eq!(read_record(&mut r).await.unwrap().unwrap(), "valid");

        // CRLF max and max+1
        let input_crlf_ok = format!("{}\r\nvalid", "b".repeat(MAX_RECORD_SIZE));
        let mut r = BufReader::new(input_crlf_ok.as_bytes());
        assert_eq!(
            read_record(&mut r).await.unwrap().unwrap().len(),
            MAX_RECORD_SIZE
        );
        assert_eq!(read_record(&mut r).await.unwrap().unwrap(), "valid");

        let input_crlf_err = format!("{}\r\nvalid", "b".repeat(MAX_RECORD_SIZE + 1));
        let mut r = BufReader::new(input_crlf_err.as_bytes());
        assert!(matches!(
            read_record(&mut r).await,
            Err(TransportError::Oversized)
        ));
        assert_eq!(read_record(&mut r).await.unwrap().unwrap(), "valid");

        // EOF without newline max and max+1
        let input_eof_ok = "c".repeat(MAX_RECORD_SIZE);
        let mut r = BufReader::new(input_eof_ok.as_bytes());
        assert_eq!(
            read_record(&mut r).await.unwrap().unwrap().len(),
            MAX_RECORD_SIZE
        );
        assert!(read_record(&mut r).await.unwrap().is_none());

        let input_eof_err = "c".repeat(MAX_RECORD_SIZE + 1);
        let mut r = BufReader::new(input_eof_err.as_bytes());
        assert!(matches!(
            read_record(&mut r).await,
            Err(TransportError::Oversized)
        ));
    }

    #[tokio::test]
    async fn invalid_utf8_rejected() {
        let invalid = vec![0xFF, 0xFE, b'\n'];
        let mut reader = BufReader::new(invalid.as_slice());
        assert!(matches!(
            read_record(&mut reader).await,
            Err(TransportError::Utf8)
        ));
    }
}
