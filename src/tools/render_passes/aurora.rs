use super::*;

#[derive(Default)]
pub struct NoAsteroid;
impl RenderPass for NoAsteroid {
    fn path_filter(&self, path: &str) -> bool {
        !subpath(path, "/turf/unsimulated/chasm_mask") &&
        !subpath(path, "/turf/unsimulated/floor/asteroid/ash/rocky") &&
        !subpath(path, "/turf/simulated/mineral")
    }
}

#[derive(Default)]
pub struct StationOnly;
impl RenderPass for StationOnly {
    fn late_filter_turf(&self, atom: &Atom, objtree: &ObjectTree) -> bool {
        if atom.istype("/area/") {
            if atom.get_var("station_area", objtree).to_bool() {
                return true;
            }
            return false;
        }
        true
    }
}