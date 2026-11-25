// Collect notices for the Windows binary from the same packages Cargo resolved.
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";

const target = execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (.+)$/m)[1].trim();
const metadata = JSON.parse(execFileSync("cargo", [
  "metadata", "--locked", "--format-version", "1", "--filter-platform", target,
], { encoding: "utf8", maxBuffer: 32 * 1024 * 1024 }));
const nodes = new Map(metadata.resolve.nodes.map(node => [node.id, node]));
const used = new Set();
function visit(id) {
  if (used.has(id)) return;
  used.add(id);
  for (const dep of nodes.get(id).deps) {
    if (dep.dep_kinds.some(edge => edge.kind === null)) visit(dep.pkg);
  }
}
visit(metadata.resolve.root);

const texts = new Map();
function add(label, file) {
  const body = readFileSync(file, "utf8").replace(/\r\n/g, "\n").trim();
  if (!texts.has(body)) texts.set(body, new Set());
  texts.get(body).add(label);
}
for (const pkg of metadata.packages) {
  if (!used.has(pkg.id) || pkg.id === metadata.resolve.root) continue;
  const dir = dirname(pkg.manifest_path);
  const files = readdirSync(dir, { withFileTypes: true })
    .filter(f => f.isFile() && /^(licen[sc]e|notice|copying|unlicense)(?:$|[.-])/i.test(f.name))
    .map(f => join(dir, f.name));
  if (pkg.license_file) files.push(join(dir, pkg.license_file));
  if (!files.length && pkg.repository === "https://github.com/wravery/webview2-rs") {
    files.push("assets/licenses/webview2-rs.txt");
  }
  if (!files.length) throw new Error("缺少依赖许可文件 " + pkg.name);
  for (const file of files) add(pkg.name + " " + pkg.version, file);
}
for (const name of ["LICENSE", "NOTICE"]) {
  add("Microsoft Edge WebView2 SDK", "assets/licenses/WebView2-SDK-" + name + ".txt");
}
const frontend = readFileSync("dist/ui-notices.md", "utf8");
const native = [...texts].map(([body, labels]) => [...labels].join(", ") + "\n\n" + body);
writeFileSync("dist/notices.txt", [frontend, ...native].join("\n\n----------------\n\n"), "utf8");
