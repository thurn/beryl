import { spawnSync } from "node:child_process";

// Ignite launches commands directly. Windows supplies npm as a command script.
const [program, args] =
    process.platform === "win32"
        ? ["cmd.exe", ["/d", "/c", "npm.cmd", "ci"]]
        : ["npm", ["ci"]];
const run = spawnSync(program, args, { stdio: "inherit" });
if (run.error) console.error(run.error);
process.exit(run.status ?? 1);
