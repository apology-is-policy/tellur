const esbuild = require('esbuild');
esbuild.buildSync({ entryPoints: ['extension.cjs'], bundle: true, outfile: 'dist/extension.js', platform: 'node', format: 'cjs', external: ['vscode'], sourcemap: true });
