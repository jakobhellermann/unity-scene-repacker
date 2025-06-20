#![allow(non_snake_case)]
use rabex::objects::ClassId;

pub mod pptr;
pub mod types;

pub trait ClassIdType {
    const CLASS_ID: ClassId;
}
