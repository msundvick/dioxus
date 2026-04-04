/// Middle layer — depends on bin-dep-nested, called by bin-transitive-dep.
pub fn quadruple(x: u32) -> u32 {
    bin_dep_nested::double(bin_dep_nested::double(x))
}
