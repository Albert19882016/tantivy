use core::index::{Segment, SegmentId};
use core::schema::Term;
use core::store::StoreReader;
use core::schema::Document;
use core::postings::IntersectionPostings;
use core::directory::ReadOnlySource;
use std::io::Cursor;
use core::schema::DocId;
use core::index::SegmentComponent;
use core::postings::Postings;
use core::simdcompression::Decoder;
use std::io;
use std::str;
use core::codec::TermInfo;
use core::fstmap::FstMap;
use std::fmt;
use rustc_serialize::json;
use core::codec::SegmentSerializer;
use core::index::SerializableSegment;
use core::index::SegmentInfo;
use core::convert_to_ioerror;
use core::serialize::BinarySerializable;

// TODO file structure should be in codec

pub struct SegmentReader {
    segment_info: SegmentInfo,
    segment_id: SegmentId,
    term_offsets: FstMap<TermInfo>,
    postings_data: ReadOnlySource,
    store_reader: StoreReader,
}


impl fmt::Debug for SegmentReader {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "SegmentReader({:?})", self.segment_id)
    }
}

pub struct SegmentPostings {
    doc_id: usize,
    doc_ids: Vec<u32>,
}

impl SegmentPostings {

    pub fn empty()-> SegmentPostings {
        SegmentPostings {
            doc_id: 0,
            doc_ids: Vec::new(),
        }
    }

    pub fn from_data(data: &[u8]) -> SegmentPostings {
        let mut cursor = Cursor::new(data);
        let data: Vec<u32> = Vec::deserialize(&mut cursor).unwrap();
        let mut doc_ids: Vec<u32> = (0u32..data.len() as u32 + 1_000 as u32).collect();
        let decoder = Decoder::new();
        let num_doc_ids = decoder.decode_sorted(&data, &mut doc_ids);
        doc_ids.truncate(num_doc_ids);
        SegmentPostings {
            doc_ids: doc_ids,
            doc_id: 0,
        }
    }

}

impl Postings for SegmentPostings {
    fn skip_next(&mut self, target: DocId) -> Option<DocId> {
        loop {
            match Iterator::next(self) {
                Some(val) if val >= target => {
                    return Some(val);
                },
                None => {
                    return None;
                },
                _ => {}
            }
        }
    }
}


impl Iterator for SegmentPostings {

    type Item = DocId;

    fn next(&mut self,) -> Option<DocId> {
        if self.doc_id < self.doc_ids.len() {
            let res = Some(self.doc_ids[self.doc_id]);
            self.doc_id += 1;
            return res;
        }
        else {
            None
        }
    }
}


impl SegmentReader {

    /// Returns the highest document id ever attributed in
    /// this segment + 1.
    /// Today, `tantivy` does not handle deletes so, it happens
    /// to also be the number of documents in the index.
    pub fn max_doc(&self,) -> DocId {
        self.segment_info.max_doc
    }


    /// Open a new segment for reading.
    pub fn open(segment: Segment) -> io::Result<SegmentReader> {
        let segment_info_reader = try!(segment.open_read(SegmentComponent::INFO));
        let segment_info_data = try!(str::from_utf8(&*segment_info_reader).map_err(convert_to_ioerror));
        let segment_info: SegmentInfo = try!(json::decode(&segment_info_data).map_err(convert_to_ioerror));
        let source = try!(segment.open_read(SegmentComponent::TERMS));
        let term_offsets = try!(FstMap::from_source(source));
        let store_reader = StoreReader::new(try!(segment.open_read(SegmentComponent::STORE)));
        let postings_shared_mmap = try!(segment.open_read(SegmentComponent::POSTINGS));
        Ok(SegmentReader {
            segment_info: segment_info,
            postings_data: postings_shared_mmap,
            term_offsets: term_offsets,
            segment_id: segment.id(),
            store_reader: store_reader,
        })
    }

    /// Returns the document (or to be accurate, its stored field)
    /// bearing the given doc id.
    /// This method is slow and should seldom be called from
    /// within a collector.
    pub fn  doc(&self, doc_id: &DocId) -> io::Result<Document> {
        self.store_reader.get(doc_id)
    }

    fn read_postings(&self, offset: u32) -> SegmentPostings {
        let postings_data = &self.postings_data.as_slice()[(offset as usize)..];
        SegmentPostings::from_data(&postings_data)
    }

    fn get_term<'a>(&'a self, term: &Term) -> Option<TermInfo> {
        self.term_offsets.get(term.as_slice())
    }

    /// Returns the list of doc ids containing all of the
    /// given terms.
    pub fn search(&self, terms: &Vec<Term>) -> IntersectionPostings<SegmentPostings> {

        let mut segment_postings: Vec<SegmentPostings> = Vec::new();
        for term in terms.iter() {
            match self.get_term(term) {
                Some(term_info) => {
                    let segment_posting = self.read_postings(term_info.postings_offset);
                    segment_postings.push(segment_posting);
                }
                None => {
                    segment_postings.clear();
                    segment_postings.push(SegmentPostings::empty());
                    break;
                }
            }
        }
        IntersectionPostings::from_postings(segment_postings)
    }

}


impl SerializableSegment for SegmentReader {

    fn write(&self, mut serializer: SegmentSerializer) -> io::Result<()> {
        let mut term_offsets_it = self.term_offsets.stream();
        loop {
            match term_offsets_it.next() {
                Some((term_data, term_info)) => {
                    let term = Term::from(term_data);
                    try!(serializer.new_term(&term, term_info.doc_freq));
                    let segment_postings = self.read_postings(term_info.postings_offset);
                    try!(serializer.write_docs(&segment_postings.doc_ids[..]));
                },
                None => { break; }
            }
        }
        for doc_id in 0..self.max_doc() {
            let doc = try!(self.store_reader.get(&doc_id));
            try!(serializer.store_doc(&mut doc.text_fields()));
        }
        serializer.close()
    }
}
