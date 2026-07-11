import { readdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

async function normalizeDirectory(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      await normalizeDirectory(path);
    } else if (entry.name.endsWith(".d.ts")) {
      const source = await readFile(path, "utf8");
      await writeFile(path, source.replaceAll('.ts"', '.js"'));
    }
  }
}

const directories = process.argv.slice(2);
for (const directory of directories.length === 0 ? ["dist"] : directories) {
  await normalizeDirectory(directory);
}
