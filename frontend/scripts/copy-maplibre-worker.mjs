// MapLibre v6 charge son worker à côté de son module. Une fois empaqueté par Vite, ce chemin
// n'existe plus : on copie donc le worker dans public/ et on l'indique via setWorkerUrl (voir MapView).
import { copyFileSync, mkdirSync } from "node:fs";

const src = new URL("../node_modules/maplibre-gl/dist/", import.meta.url);
const dest = new URL("../public/maplibre/", import.meta.url);
mkdirSync(dest, { recursive: true });
for (const f of ["maplibre-gl-worker.mjs", "maplibre-gl-shared.mjs"]) {
  copyFileSync(new URL(f, src), new URL(f, dest));
}
