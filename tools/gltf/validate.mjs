import fs from 'fs'; import validator from 'gltf-validator';
const b = fs.readFileSync(process.argv[2]);
const r = await validator.validateBytes(new Uint8Array(b));
console.log(JSON.stringify(r.issues,null,1).slice(0,1500)); console.log(JSON.stringify(r.info,null,0).slice(0,600));
