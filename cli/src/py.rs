use pyo3::prelude::*;

#[pyfunction]
fn entrypoint() -> PyResult<()> {
    crate::main(std::env::args_os().skip(1).collect());

    Ok(())
}

#[pymodule(name = "unity_scene_repacker")]
fn my_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_wrapped(wrap_pyfunction!(entrypoint))
}
