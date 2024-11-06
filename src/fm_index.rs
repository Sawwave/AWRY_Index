use crate::{
    alphabet::{alphabet_cardinality, Symbol, SymbolAlphabet},
    bwt::{AminoBwtBlock, NucleotideBwtBlock},
    search::{SearchPtr, SearchRange},
};
use bwt;
use bwt::Bwt;

pub struct FmIndex {
    bwt: Bwt,
    prefix_sums: Vec<u64>,
    sampled_suffix_array: Vec<u64>,
    suffix_array_compression_ratio: usize,
}

const SUFFIX_ARRAY_FILE_NAME: &str = "sa.sufr";

impl FmIndex {
    pub fn new(
        input_file_src: String,
        suffix_array_output_src: &String,
        bwt_alphabet: &SymbolAlphabet,
        max_query_len: Option<usize>,
        threads: Option<usize>,
        suffix_array_compression_ratio: usize,
        alphabet: SymbolAlphabet,
    ) -> Result<Self, anyhow::Error> {
        let create_args = sufr::CreateArgs {
            input: input_file_src,
            num_partitions: 16, //this is the default, so okay I guess?
            max_query_len,
            threads,
            output: Some(suffix_array_output_src.clone()),
            is_dna: alphabet == SymbolAlphabet::Nucleotide,
            allow_ambiguity: true,
            ignore_softmask: true,
        };
        sufr::create(&create_args);

        let mut sampled_suffix_array: Vec<u64> = Vec::new();
        let suffix_array_file: libsufr::SufrFile<u64> =
            libsufr::SufrFile::read(&suffix_array_output_src)?;
        let bwt_length = suffix_array_file.num_suffixes;

        //find the number of blocks needed (integer ceiling funciton)
        let num_bwt_blocks = bwt_length.div_ceil(bwt::NUM_POSITIONS_PER_BLOCK);

        let mut bwt = match bwt_alphabet {
            SymbolAlphabet::Nucleotide => {
                Bwt::Nucleotide(vec![NucleotideBwtBlock::new(); num_bwt_blocks as usize])
            }
            SymbolAlphabet::Amino => {
                Bwt::Amino(vec![AminoBwtBlock::new(); num_bwt_blocks as usize])
            }
        };

        let alphabet_cardinality = alphabet_cardinality(&bwt_alphabet);
        let mut letter_counts = vec![0; alphabet_cardinality as usize];

        let mut suffix_array = suffix_array_file.suffix_array;
        for (suffix_idx, suffix_array_value) in suffix_array.iter().enumerate() {
            //generate the sampled suffix array
            if suffix_idx % suffix_array_compression_ratio == 0 {
                sampled_suffix_array.push(suffix_array_value);
            }
            //set the block milestones, if necessary
            if suffix_array_value % bwt::NUM_POSITIONS_PER_BLOCK == 0 {
                bwt.set_milestones(
                    (suffix_array_value / bwt::NUM_POSITIONS_PER_BLOCK) as usize,
                    &letter_counts,
                );
            }
            suffix_array_file.text[0];
            // get the letter immediately before the suffix array
            let preceeding_letter_ascii = match suffix_idx {
                0 => '$',
                _ => suffix_array_file.text[(suffix_array_value - 1) as usize] as char,
            };

            let preceding_symbol = Symbol::new_ascii(bwt_alphabet.clone(), preceeding_letter_ascii);
            let preceding_letter_idx = preceding_symbol.index();
            bwt.set_symbol_at(&(suffix_idx as u64), &preceding_symbol);
            letter_counts[preceding_letter_idx as usize] += 1;
        }

        //generate the prefix sums for the letter counts
        let mut prefix_sums: Vec<u64> = vec![0; alphabet_cardinality as usize + 1];
        let mut accumulated_value = 0;
        for idx in 0..alphabet_cardinality + 1 {
            prefix_sums[idx as usize] = accumulated_value;
            if idx != alphabet_cardinality {
                accumulated_value += letter_counts[idx as usize]
            }
        }

        return Ok(FmIndex {
            bwt,
            prefix_sums,
            sampled_suffix_array,
            suffix_array_compression_ratio,
        });
    }

    pub fn len(&self) -> usize {
        return self.prefix_sums[self.prefix_sums.len() - 1] as usize;
    }

    pub fn count_string(&self, query: &String) -> usize {
        let mut search_range = SearchRange::new(self);

        for query_char in query.chars().rev() {
            search_range = self.update_range_with_char(search_range, &query_char);
            if search_range.is_empty() {
                return 0;
            }
        }

        return 0;
    }

    pub fn update_range_with_char(
        &self,
        search_range: SearchRange,
        query_char: &char,
    ) -> SearchRange {
        let alphabet = match &self.bwt {
            Bwt::Nucleotide(_) => &SymbolAlphabet::Nucleotide,
            Bwt::Amino(_) => &SymbolAlphabet::Amino,
        };
        let query_symbol = Symbol::new_ascii(alphabet.clone(), *query_char);
        let query_symbol_idx = query_symbol.index() as usize;
        let letter_prefix_sum = self.prefix_sums[query_symbol_idx];
        let new_start_ptr = self
            .bwt
            .global_occurrence(search_range.start_ptr, &query_symbol);
        let new_end_ptr = letter_prefix_sum
            + self
                .bwt
                .global_occurrence(search_range.end_ptr, &query_symbol)
            - 1;

        SearchRange {
            start_ptr: new_start_ptr,
            end_ptr: new_end_ptr,
        }
    }

    pub fn backstep(&self, search_pointer: SearchPtr) -> SearchPtr {
        let symbol = self.bwt.get_symbol_at(&search_pointer);
        let symbol_prefix_sum = self.prefix_sums[symbol.index() as usize];
        let global_occurrence = self.bwt.global_occurrence(search_pointer, &symbol);
        return symbol_prefix_sum + global_occurrence - 1;
    }
}