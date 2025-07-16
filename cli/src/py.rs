use std::path::PathBuf;

use pyo3::prelude::*;

#[pyfunction]
fn entrypoint(py: Python) -> PyResult<()> {
    let platlib = py
        .import("sysconfig")?
        .getattr("get_path")?
        .call1(("platlib",))?
        .extract::<PathBuf>()?;
    let libs = platlib.join("unity_scene_repacker.libs");
    crate::main(std::env::args_os().skip(1).collect(), Some(&libs));

    Ok(())
}

#[pymodule(name = "unity_scene_repacker")]
fn my_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_wrapped(wrap_pyfunction!(entrypoint))
}
