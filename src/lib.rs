pub mod compress;
pub mod decompress;
pub mod util;

#[cfg(test)]
mod tests {
    use std::{fs, io::Cursor};

    use sha1::{Digest, Sha1};

    use crate::{compress, decompress};

    #[allow(dead_code)]
    //#[test] // uncomment to generate compressed test files
    fn test_comp_uncomp() {
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

    #[test]
    fn compress_decompress_test() {
        let files = fs::read_dir("./test-data/uncompressed").unwrap();

        for file in files {
            let data = fs::read(file.unwrap().path()).unwrap();

            let mut sha = Sha1::new();
            sha.update(data.clone());
            let uncomp_data_sha = sha.finalize();

            let data_compressed = compress::compress(&data).unwrap();

            let data_uncompressed =
                decompress::decompress_reader(&mut Cursor::new(data_compressed), None).unwrap();

            sha = Sha1::new();
            sha.update(data_uncompressed);
            let comp_data_sha = sha.finalize();
            assert_eq!(uncomp_data_sha, comp_data_sha);
        }
    }

    #[test]
    fn decompress_test() {
        let files = fs::read_dir("./test-data/compressed").unwrap();

        for file in files {
            let data = fs::read(file.as_ref().unwrap().path()).unwrap();

            let data_uncompressed =
                decompress::decompress_reader(&mut Cursor::new(data), None).unwrap();

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

            let data_compressed = compress::compress(&data).unwrap();

            let data_uncompressed = lzo.decompress_safe(&data_compressed, data_len).unwrap();

            sha = Sha1::new();
            sha.update(data_uncompressed);
            let comp_data_sha = sha.finalize();
            assert_eq!(uncomp_data_sha, comp_data_sha);
        }
    }

    #[test]
    fn decompress_test_small() {
        let compressed = fs::read("./test-data/compressed/pic_small.png.lzo").unwrap();

        // let size =
        //     decompress::decompress_stream(&mut Cursor::new(compressed.clone()), Some(442780))
        //         .unwrap();

        let size2 = decompress::decompress_reader(&mut Cursor::new(compressed), None).unwrap();
        fs::write("./test-data/output/pic_small.out.png", size2).unwrap();
    }
}
