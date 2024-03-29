//! # LZ👌in Rust
//!
//! A pure rust port of [lzokay](https://github.com/jackoalan/lzokay), which is a C++ implementation of the [LZO compression format](http://www.oberhumer.com/opensource/lzo/).
//!
//! ## Examples
//!
//! ### Compressing raw data
//! ```rust
//! use std::fs::File;
//!
//! fn main () -> Result<(), lzokay_native::Error> {
//!     let data = include_bytes!("../test-data/uncompressed/alice29.txt");
//!     let compressed = lzokay_native::compress(data);
//!
//!     Ok(())
//! }
//! ```
//!
//! ### Multiple compressions without repeated (de-)allocations
//! By using a [`Dict`] and [`compress_with_dict`] you can avoid repeat allocation/deallocation
//! of the work memory used by the compressor:
//!
//! ```rust
//! use std::fs::File;
//!
//! fn main () -> Result<(), lzokay_native::Error> {
//!     let mut dict = lzokay_native::Dict::new();
//!
//!     let data1 = include_bytes!("../test-data/uncompressed/alice29.txt");
//!     let compressed1 = lzokay_native::compress_with_dict(data1, &mut dict)?;
//!
//!     let data2 = include_bytes!("../test-data/uncompressed/asyoulik.txt");
//!     let compressed2 = lzokay_native::compress_with_dict(data2, &mut dict)?;
//!     
//!     Ok(())
//! }
//! ```
//!
//! ### Decompressing a file
//! ```rust
//! use std::fs::File;
//!
//! fn main () -> Result<(), lzokay_native::Error> {
//!     let file_path = "./test-data/compressed/fields.c.lzo";
//!     
//!     let mut file = File::open(file_path)?; // file implements std::io::Read
//!     let decompressed = lzokay_native::decompress(&mut file, None);
//!
//!     Ok(())
//! }
//! ```
//!
//! ### Decompressing raw data
//! ```rust
//! use std::fs::File;
//!
//! fn main () -> Result<(), lzokay_native::Error> {
//!     let data = include_bytes!("../test-data/compressed/fields.c.lzo");
//!     let decompressed = lzokay_native::decompress_all(data, None);
//!
//!     Ok(())
//! }
//! ```

#![warn(missing_docs)]
#![warn(clippy::cargo)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::unwrap_used)]
#![warn(clippy::expect_used)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::multiple_crate_versions)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]

#[cfg(feature = "compress")]
mod compress;
#[cfg(feature = "compress")]
pub use compress::*;

#[cfg(feature = "decompress")]
mod decompress;
#[cfg(feature = "decompress")]
pub use decompress::*;

mod util;

pub use util::Error;

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(any(feature = "decompress", feature = "compress"))]
    use std::io::Cursor;

    #[cfg(any(feature = "decompress", feature = "compress"))]
    use sha1::Digest;
    #[cfg(any(feature = "decompress", feature = "compress"))]
    use sha1::Sha1;

    #[test]
    #[ignore]
    fn generated_test_data() {
        let files = fs::read_dir("./test-data/uncompressed").unwrap();

        let mut lzo = minilzo_rs::LZO::init().unwrap();

        for file in files {
            let data = fs::read(file.as_ref().unwrap().path()).unwrap();

            let compressed_data = lzo.compress(&data).unwrap();

            let mut output_path = "./test-data/compressed/".to_owned();
            output_path.push_str(file.unwrap().file_name().to_str().unwrap());
            output_path.push_str(".lzo");
            fs::write(output_path, compressed_data).unwrap();
        }
    }

    #[cfg(feature = "compress")]
    #[test]
    fn compress_decompress_test() {
        let files = fs::read_dir("./test-data/uncompressed").unwrap();

        for file in files {
            let data = fs::read(file.unwrap().path()).unwrap();

            let mut sha = Sha1::new();
            sha.update(data.clone());
            let uncomp_data_sha = sha.finalize();

            let data_compressed = crate::compress::compress(&data).unwrap();

            let data_uncompressed =
                crate::decompress::decompress(&mut Cursor::new(data_compressed), None).unwrap();

            sha = Sha1::new();
            sha.update(data_uncompressed);
            let comp_data_sha = sha.finalize();
            assert_eq!(uncomp_data_sha, comp_data_sha);
        }
    }

    #[cfg(feature = "decompress")]
    #[test]
    fn decompress_test() {
        let files = fs::read_dir("./test-data/compressed").unwrap();

        for file in files {
            let data = fs::read(file.as_ref().unwrap().path()).unwrap();

            let data_uncompressed =
                crate::decompress::decompress(&mut Cursor::new(data), None).unwrap();

            let mut sha = Sha1::new();
            sha.update(data_uncompressed);
            let uncomp_data_sha = sha.finalize();

            let file_name = file
                .unwrap()
                .path()
                .file_stem()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();

            let mut input_path = "./test-data/uncompressed/".to_owned();
            input_path.push_str(&file_name);

            let comp_data = fs::read(input_path).unwrap();
            sha = Sha1::new();
            sha.update(comp_data);
            let comp_data_sha = sha.finalize();

            assert_eq!(uncomp_data_sha, comp_data_sha);
        }
    }

    #[cfg(feature = "compress")]
    #[test]
    fn check_lzo_decompress_compatibility() {
        let files = fs::read_dir("./test-data/uncompressed").unwrap();

        let lzo = minilzo_rs::LZO::init().unwrap();

        for file in files {
            let data = fs::read(file.unwrap().path()).unwrap();
            let data_len = data.len();

            let mut sha = Sha1::new();
            sha.update(data.clone());
            let uncomp_data_sha = sha.finalize();

            let data_compressed = crate::compress::compress(&data).unwrap();

            let data_uncompressed = lzo.decompress_safe(&data_compressed, data_len).unwrap();

            sha = Sha1::new();
            sha.update(data_uncompressed);
            let comp_data_sha = sha.finalize();
            assert_eq!(uncomp_data_sha, comp_data_sha);
        }
    }

    #[cfg(feature = "decompress")]
    #[test]
    fn decompress_test_small() {
        let compressed = fs::read("./test-data/compressed/pic_small.png.lzo").unwrap();

        // let size =
        //     decompress::decompress_stream(&mut Cursor::new(compressed.clone()), Some(442780))
        //         .unwrap();

        let size2 = crate::decompress::decompress(&mut Cursor::new(compressed), None).unwrap();
        fs::write("./test-data/output/pic_small.out.png", size2).unwrap();
    }
}
