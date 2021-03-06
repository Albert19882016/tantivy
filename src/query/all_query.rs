use core::Searcher;
use core::SegmentReader;
use docset::DocSet;
use query::Query;
use query::Scorer;
use query::Weight;
use DocId;
use Result;
use Score;

/// Query that matches all of the documents.
///
/// All of the document get the score 1f32.
#[derive(Debug)]
pub struct AllQuery;

impl Query for AllQuery {
    fn weight(&self, _: &Searcher, _: bool) -> Result<Box<Weight>> {
        Ok(Box::new(AllWeight))
    }
}

/// Weight associated to the `AllQuery` query.
pub struct AllWeight;

impl Weight for AllWeight {
    fn scorer(&self, reader: &SegmentReader) -> Result<Box<Scorer>> {
        Ok(Box::new(AllScorer {
            started: false,
            doc: 0u32,
            max_doc: reader.max_doc(),
        }))
    }
}

/// Scorer associated to the `AllQuery` query.
pub struct AllScorer {
    started: bool,
    doc: DocId,
    max_doc: DocId,
}

impl DocSet for AllScorer {
    fn advance(&mut self) -> bool {
        if self.started {
            self.doc += 1u32;
        } else {
            self.started = true;
        }
        self.doc < self.max_doc
    }

    fn doc(&self) -> DocId {
        self.doc
    }

    fn size_hint(&self) -> u32 {
        self.max_doc
    }
}

impl Scorer for AllScorer {
    fn score(&mut self) -> Score {
        1f32
    }
}
