import * as path from "https://deno.land/std@0.208.0/path/mod.ts";

export interface Manifest {
  bundle_name: string;
  runtime: string;
  version: string;
  file: string;
  provides: string[];
  function_count: Record<string, number>;
}

export interface Bundle {
  path: string;
  manifest: Manifest;
}

export function scanDir(dirPath: string): Bundle[] {
  const bundles: Bundle[] = [];
  
  for (const entry of Deno.readDirSync(dirPath)) {
    if (!entry.isDirectory) continue;
    
    const manifestPath = path.join(dirPath, entry.name, "manifest.toml");
    try {
      const content = Deno.readTextFileSync(manifestPath);
      const manifest = parseToml(content) as Manifest;
      bundles.push({
        path: path.join(dirPath, entry.name),
        manifest,
      });
    } catch {
      // No manifest.toml in this directory
    }
  }
  
  return bundles;
}

// Simple TOML parser for manifest files
function parseToml(content: string): Record<string, any> {
  const result: Record<string, any> = {};
  let currentSection: string | null = null;
  
  for (const line of content.split('\n')) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('#')) continue;
    
    // Section header [section] or [section.subsection]
    const sectionMatch = trimmed.match(/^\[(.+)\]$/);
    if (sectionMatch) {
      currentSection = sectionMatch[1];
      if (!result[currentSection]) {
        result[currentSection] = {};
      }
      continue;
    }
    
    // Key-value pair
    const kvMatch = trimmed.match(/^(\w+)\s*=\s*(.+)$/);
    if (kvMatch) {
      const key = kvMatch[1];
      let value = kvMatch[2].trim();
      
      // Parse value
      if (value.startsWith('"') && value.endsWith('"')) {
        value = value.slice(1, -1);
      } else if (value.startsWith('[') && value.endsWith(']')) {
        // Array
        value = value.slice(1, -1).split(',').map(s => s.trim().replace(/^"|"$/g, ''));
      } else if (/^\d+$/.test(value)) {
        value = parseInt(value, 10);
      }
      
      if (currentSection) {
        result[currentSection][key] = value;
      } else {
        result[key] = value;
      }
    }
  }
  
  return result;
}
