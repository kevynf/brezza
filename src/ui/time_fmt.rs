use jiff::tz::TimeZone;
use jiff::{Timestamp, Zoned};

use crate::fl;

pub fn short(time: Timestamp) -> String {
    let tz = TimeZone::system();
    let now = Timestamp::now().to_zoned(tz.clone());
    let z = time.to_zoned(tz);
    if z.date() == now.date() {
        clock(&z)
    } else if z.year() == now.year() {
        fl!(
            "date-this-year",
            wd = z.weekday().to_monday_one_offset(),
            month = z.month(),
            day = z.day(),
            time = clock(&z)
        )
    } else {
        fl!(
            "date-older",
            month = z.month(),
            day = z.day(),
            year = z.year()
        )
    }
}

pub fn full(time: Timestamp) -> String {
    let z = time.to_zoned(TimeZone::system());
    fl!(
        "date-full",
        wd = z.weekday().to_monday_one_offset(),
        month = z.month(),
        day = z.day(),
        year = z.year(),
        time = clock(&z)
    )
}

fn clock(z: &Zoned) -> String {
    z.strftime("%H:%M").to_string()
}
