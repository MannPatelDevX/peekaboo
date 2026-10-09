// Builds peekaboo-hook and places it where Tauri's externalBin expects it.
import { execSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";

const release = process.argv.includes("--release");
const profile = release ? "release" : "debug";
const host = execSync("rustc -vV").toString().match(/host: (\S+)/)[1];
const target = process.env.TAURI_ENV_TARGET_TRIPLE || host;
const ext = target.includes("windows") ? ".exe" : "";
const out = `src-tauri/binaries/peekaboo-hook-${target}${ext}`;
mkdirSync("src-tauri/binaries", { recursive: true });

const build = (triple) => {
  execSync(`cargo build -p peekaboo-hook ${release ? "--release" : ""} --target ${triple}`, { stdio: "inherit" });
  return `target/${triple}/${profile}/peekaboo-hook${ext}`;
};

if (target === "universal-apple-darwin") {
  const parts = ["aarch64-apple-darwin", "x86_64-apple-darwin"].map(build);
  execSync(`lipo -create -output ${out} ${parts.join(" ")}`, { stdio: "inherit" });
} else {
  copyFileSync(build(target), out);
}
console.log(`sidecar ready: ${out}`);
