// Reuse only our known loopback development reader; never terminate a prior server.
import { spawn } from 'node:child_process';
try {
  const response = await fetch('http://127.0.0.1:43182/', { signal: AbortSignal.timeout(2000) });
  if (response.ok && (await response.text()).includes('<title>AI-Native PDF Reader</title>')) process.exit(0);
  throw Error('Port 43182 belongs to an unexpected service');
} catch (error) {
  if (error.message === 'Port 43182 belongs to an unexpected service') throw error;
}
const child = spawn(process.execPath, ['node_modules/vite/bin/vite.js'], { stdio: 'inherit', windowsHide: true });
child.on('exit', code => { process.exitCode = code ?? 1; });
