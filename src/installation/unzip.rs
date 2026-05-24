use std::{
    fs::{self, File},
    io::{BufReader, Read},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::installation::only_one_file_in_dir;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveFormat {
    // Tar + compression
    Tar,
    TarGzip,
    TarBzip2,
    TarBzip3,
    TarLz4,
    TarXz,
    TarLzma,
    TarLzip,
    TarSnappy,
    TarZstd,
    // Other archive formats
    Zip,
    SevenZip,
    // Standalone compression (non-archive, single file)
    Gzip,
    Bzip2,
    Bzip3,
    Lz4,
    Xz,
    Lzma,
    Lzip,
    Snappy,
    Zstd,
    Brotli,
}

impl ArchiveFormat {
    fn from_path(path: &Path) -> Option<Self> {
        let filename = path.file_name()?.to_str()?.to_ascii_lowercase();
        Self::from_filename(&filename)
    }

    fn from_filename(filename: &str) -> Option<Self> {
        let filename = filename.to_ascii_lowercase();
        let filename = filename.as_str();
        // Compound extensions (tar + compression) checked first
        match filename {
            f if f.ends_with(".tar.gz") || f.ends_with(".tgz") => return Some(Self::TarGzip),
            f if f.ends_with(".tar.bz2")
                || f.ends_with(".tar.bz")
                || f.ends_with(".tbz2")
                || f.ends_with(".tbz") =>
            {
                return Some(Self::TarBzip2);
            }
            f if f.ends_with(".tar.bz3") || f.ends_with(".tbz3") => return Some(Self::TarBzip3),
            f if f.ends_with(".tar.lz4") || f.ends_with(".tlz4") => return Some(Self::TarLz4),
            f if f.ends_with(".tar.xz") || f.ends_with(".txz") => return Some(Self::TarXz),
            f if f.ends_with(".tar.lzma") || f.ends_with(".tlzma") => return Some(Self::TarLzma),
            f if f.ends_with(".tar.lz") || f.ends_with(".tlz") => return Some(Self::TarLzip),
            f if f.ends_with(".tar.sz") || f.ends_with(".tsz") => return Some(Self::TarSnappy),
            f if f.ends_with(".tar.zst") || f.ends_with(".tzst") => return Some(Self::TarZstd),
            _ => {}
        }
        // Archive formats
        match filename {
            f if f.ends_with(".zip") || f.ends_with(".cbz") => return Some(Self::Zip),
            f if f.ends_with(".tar") || f.ends_with(".cbt") => return Some(Self::Tar),
            f if f.ends_with(".7z") || f.ends_with(".cb7") => return Some(Self::SevenZip),
            _ => {}
        }
        // Standalone compression formats
        match filename {
            f if f.ends_with(".gz") => return Some(Self::Gzip),
            f if f.ends_with(".bz2") || f.ends_with(".bz") => return Some(Self::Bzip2),
            f if f.ends_with(".bz3") => return Some(Self::Bzip3),
            f if f.ends_with(".lz4") => return Some(Self::Lz4),
            f if f.ends_with(".xz") => return Some(Self::Xz),
            f if f.ends_with(".lzma") => return Some(Self::Lzma),
            f if f.ends_with(".lz") => return Some(Self::Lzip),
            f if f.ends_with(".sz") => return Some(Self::Snappy),
            f if f.ends_with(".zst") => return Some(Self::Zstd),
            f if f.ends_with(".br") => return Some(Self::Brotli),
            _ => {}
        }
        None
    }

    fn is_archive(&self) -> bool {
        matches!(
            self,
            Self::Tar
                | Self::TarGzip
                | Self::TarBzip2
                | Self::TarBzip3
                | Self::TarLz4
                | Self::TarXz
                | Self::TarLzma
                | Self::TarLzip
                | Self::TarSnappy
                | Self::TarZstd
                | Self::Zip
                | Self::SevenZip
        )
    }
}

fn extract_tar_gz(reader: impl Read, output: &Path) -> Result<()> {
    let decoder = flate2::read::MultiGzDecoder::new(reader);
    tar::Archive::new(decoder).unpack(output)?;
    Ok(())
}

fn extract_tar_bz2(reader: impl Read, output: &Path) -> Result<()> {
    let decoder = bzip2::read::MultiBzDecoder::new(reader);
    tar::Archive::new(decoder).unpack(output)?;
    Ok(())
}

fn extract_tar_bz3(reader: impl Read, output: &Path) -> Result<()> {
    let decoder = bzip3::read::Bz3Decoder::new(reader)?;
    tar::Archive::new(decoder).unpack(output)?;
    Ok(())
}

fn extract_tar_lz4(reader: impl Read, output: &Path) -> Result<()> {
    let decoder = MultiFrameLz4Decoder::new(reader);
    tar::Archive::new(decoder).unpack(output)?;
    Ok(())
}

fn extract_tar_xz(reader: impl Read, output: &Path) -> Result<()> {
    let decoder = lzma_rust2::XzReader::new(reader, true);
    tar::Archive::new(decoder).unpack(output)?;
    Ok(())
}

fn extract_tar_lzma(reader: impl Read, output: &Path) -> Result<()> {
    let decoder = lzma_rust2::LzmaReader::new_mem_limit(reader, u32::MAX, None)?;
    tar::Archive::new(decoder).unpack(output)?;
    Ok(())
}

fn extract_tar_lz(reader: impl Read, output: &Path) -> Result<()> {
    let decoder = lzma_rust2::LzipReader::new(reader);
    tar::Archive::new(decoder).unpack(output)?;
    Ok(())
}

fn extract_tar_sz(reader: impl Read, output: &Path) -> Result<()> {
    let decoder = snap::read::FrameDecoder::new(reader);
    tar::Archive::new(decoder).unpack(output)?;
    Ok(())
}

fn extract_tar_zst(reader: impl Read, output: &Path) -> Result<()> {
    let decoder = zstd::stream::Decoder::new(reader)?;
    tar::Archive::new(decoder).unpack(output)?;
    Ok(())
}

fn extract_tar(reader: impl Read, output: &Path) -> Result<()> {
    tar::Archive::new(reader).unpack(output)?;
    Ok(())
}

fn extract_zip(reader: impl Read + std::io::Seek, output: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(reader)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = match file.enclosed_name() {
            Some(path) => output.join(path),
            None => continue,
        };
        if file.is_dir() {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent()
                && !p.exists()
            {
                fs::create_dir_all(p)?;
            }
            let mut outfile = File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = file.unix_mode() {
                    fs::set_permissions(&outpath, fs::Permissions::from_mode(mode))?;
                }
            }
        }
    }
    Ok(())
}

fn extract_7z(reader: impl Read + std::io::Seek, output: &Path) -> Result<()> {
    sevenz_rust2::decompress_with_extract_fn(
        reader,
        output,
        |entry: &sevenz_rust2::ArchiveEntry,
         reader: &mut dyn Read,
         path: &PathBuf|
         -> std::result::Result<bool, sevenz_rust2::Error> {
            if entry.is_directory() {
                if !path.exists() {
                    fs::create_dir_all(path)?;
                }
            } else {
                if let Some(parent) = path.parent()
                    && !parent.exists()
                {
                    fs::create_dir_all(parent)?;
                }
                let mut file = File::create(path)?;
                std::io::copy(reader, &mut file)?;
            }
            Ok(true)
        },
    )?;
    Ok(())
}

fn extract_archive(
    reader: impl Read + std::io::Seek,
    output: &Path,
    format: ArchiveFormat,
) -> Result<()> {
    match format {
        ArchiveFormat::Tar => extract_tar(reader, output),
        ArchiveFormat::TarGzip => extract_tar_gz(reader, output),
        ArchiveFormat::TarBzip2 => extract_tar_bz2(reader, output),
        ArchiveFormat::TarBzip3 => extract_tar_bz3(reader, output),
        ArchiveFormat::TarLz4 => extract_tar_lz4(reader, output),
        ArchiveFormat::TarXz => extract_tar_xz(reader, output),
        ArchiveFormat::TarLzma => extract_tar_lzma(reader, output),
        ArchiveFormat::TarLzip => extract_tar_lz(reader, output),
        ArchiveFormat::TarSnappy => extract_tar_sz(reader, output),
        ArchiveFormat::TarZstd => extract_tar_zst(reader, output),
        ArchiveFormat::Zip => extract_zip(reader, output),
        ArchiveFormat::SevenZip => extract_7z(reader, output),
        _ => unreachable!(),
    }
}

fn decompress_single(reader: impl Read, output_file: &Path, format: ArchiveFormat) -> Result<()> {
    let mut decoder: Box<dyn Read> = match format {
        ArchiveFormat::Gzip => Box::new(flate2::read::MultiGzDecoder::new(reader)),
        ArchiveFormat::Bzip2 => Box::new(bzip2::read::MultiBzDecoder::new(reader)),
        ArchiveFormat::Bzip3 => Box::new(bzip3::read::Bz3Decoder::new(reader)?),
        ArchiveFormat::Lz4 => Box::new(MultiFrameLz4Decoder::new(reader)),
        ArchiveFormat::Xz => Box::new(lzma_rust2::XzReader::new(reader, true)),
        ArchiveFormat::Lzma => Box::new(lzma_rust2::LzmaReader::new_mem_limit(
            reader,
            u32::MAX,
            None,
        )?),
        ArchiveFormat::Lzip => Box::new(lzma_rust2::LzipReader::new(reader)),
        ArchiveFormat::Snappy => Box::new(snap::read::FrameDecoder::new(reader)),
        ArchiveFormat::Zstd => Box::new(zstd::stream::Decoder::new(reader)?),
        ArchiveFormat::Brotli => Box::new(brotli::Decompressor::new(reader, 4096)),
        _ => unreachable!(),
    };
    let mut file = File::create(output_file)?;
    std::io::copy(&mut decoder, &mut file)?;
    Ok(())
}

/// LZ4 multi-frame decoder that handles concatenated LZ4 frames.
struct MultiFrameLz4Decoder<R: Read> {
    decoder: lz4_flex::frame::FrameDecoder<R>,
}

impl<R: Read> MultiFrameLz4Decoder<R> {
    fn new(reader: R) -> Self {
        Self {
            decoder: lz4_flex::frame::FrameDecoder::new(reader),
        }
    }
}

impl<R: Read> Read for MultiFrameLz4Decoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.decoder.read(buf)? {
            0 => self.decoder.read(buf),
            bytes => Ok(bytes),
        }
    }
}

/// Extract the given archive and remove the archive file.
///
/// # Returns
///
/// Returns the "main" path of the extracted archive.
pub fn unzip(src: impl Into<PathBuf>, to: impl AsRef<Path>) -> Result<PathBuf> {
    let to = to.as_ref();
    let src = src.into();

    #[cfg(windows)]
    {
        let is_binary = src
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe") || ext.eq_ignore_ascii_case("msi"));
        if is_binary {
            fs::create_dir_all(to)?;
            fs::copy(&src, to.join(src.file_name().unwrap()))?;
            fs::remove_file(&src)?;
            return Ok(to.to_path_buf());
        }
    }

    let format = ArchiveFormat::from_path(&src)
        .with_context(|| format!("Unsupported archive format: {}", src.display()))?;

    fs::create_dir_all(to)?;

    let file = BufReader::new(File::open(&src)?);

    if format.is_archive() {
        extract_archive(file, to, format)?;
    } else {
        let output_filename = src.file_stem().unwrap_or_default();
        let output_file = to.join(output_filename);
        decompress_single(file, &output_file, format)?;
    }

    fs::remove_file(&src)?;

    if let Some(folder) = only_one_file_in_dir(to)? {
        log::debug!(
            "unwrap archive folder: {} -> {}",
            to.display(),
            folder.display()
        );
        if folder.is_dir() {
            return Ok(folder);
        }
    }

    Ok(to.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_archive_format_detection() {
        // Tar + compression
        assert_eq!(
            ArchiveFormat::from_filename("foo.tar.gz"),
            Some(ArchiveFormat::TarGzip)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.tgz"),
            Some(ArchiveFormat::TarGzip)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.tar.bz2"),
            Some(ArchiveFormat::TarBzip2)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.tbz2"),
            Some(ArchiveFormat::TarBzip2)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.tar.bz3"),
            Some(ArchiveFormat::TarBzip3)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.tbz3"),
            Some(ArchiveFormat::TarBzip3)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.tar.lz4"),
            Some(ArchiveFormat::TarLz4)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.tlz4"),
            Some(ArchiveFormat::TarLz4)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.tar.xz"),
            Some(ArchiveFormat::TarXz)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.txz"),
            Some(ArchiveFormat::TarXz)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.tar.lzma"),
            Some(ArchiveFormat::TarLzma)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.tlzma"),
            Some(ArchiveFormat::TarLzma)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.tar.lz"),
            Some(ArchiveFormat::TarLzip)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.tlz"),
            Some(ArchiveFormat::TarLzip)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.tar.sz"),
            Some(ArchiveFormat::TarSnappy)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.tsz"),
            Some(ArchiveFormat::TarSnappy)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.tar.zst"),
            Some(ArchiveFormat::TarZstd)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.tzst"),
            Some(ArchiveFormat::TarZstd)
        );
        // Archives
        assert_eq!(
            ArchiveFormat::from_filename("foo.zip"),
            Some(ArchiveFormat::Zip)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.cbz"),
            Some(ArchiveFormat::Zip)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.tar"),
            Some(ArchiveFormat::Tar)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.cbt"),
            Some(ArchiveFormat::Tar)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.7z"),
            Some(ArchiveFormat::SevenZip)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.cb7"),
            Some(ArchiveFormat::SevenZip)
        );
        // Standalone compression
        assert_eq!(
            ArchiveFormat::from_filename("foo.gz"),
            Some(ArchiveFormat::Gzip)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.bz2"),
            Some(ArchiveFormat::Bzip2)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.bz3"),
            Some(ArchiveFormat::Bzip3)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.lz4"),
            Some(ArchiveFormat::Lz4)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.xz"),
            Some(ArchiveFormat::Xz)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.lzma"),
            Some(ArchiveFormat::Lzma)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.lz"),
            Some(ArchiveFormat::Lzip)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.sz"),
            Some(ArchiveFormat::Snappy)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.zst"),
            Some(ArchiveFormat::Zstd)
        );
        assert_eq!(
            ArchiveFormat::from_filename("foo.br"),
            Some(ArchiveFormat::Brotli)
        );
        // Unknown
        assert_eq!(ArchiveFormat::from_filename("foo.exe"), None);
        assert_eq!(ArchiveFormat::from_filename("foo.txt"), None);
        // Case insensitive
        assert_eq!(
            ArchiveFormat::from_filename("FOO.ZIP"),
            Some(ArchiveFormat::Zip)
        );
    }

    #[test]
    fn test_unzip() -> Result<()> {
        let assets_dir = PathBuf::from("test_assets");
        let tempdir = tempfile::tempdir()?;
        let another_temp = tempfile::tempdir()?;
        for p in ["noroot.zip", "noroot.tar.gz", "root.tar.gz"] {
            let true_src = assets_dir.join(p);
            let src = another_temp.path().join(p);
            // Because `unzip` will remove the archive file, so we need to copy before
            // testing.
            std::fs::copy(true_src, &src)?;
            let to = tempdir.path().join(p);
            let main = unzip(src, &to)?;
            if p.starts_with("root") {
                assert_eq!(main, to.join("root"));
            } else {
                assert_eq!(main, to);
            }
        }
        Ok(())
    }
}
