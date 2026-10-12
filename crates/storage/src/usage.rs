//! Active time per site, summed by local day.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageEntry {
    pub day: NaiveDate,
    pub site: String,
    pub seconds: i64,
}
