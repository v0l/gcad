import fs from 'fs'; import * as THREE from 'three';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';
const [glbPath, clipName = 'walk', time = '0', expectedPath] = process.argv.slice(2);
const buf = fs.readFileSync(glbPath);
const ab = buf.buffer.slice(buf.byteOffset, buf.byteOffset+buf.byteLength);
const gltf = await new Promise((res,rej)=>new GLTFLoader().parse(ab,'',res,rej));
const clip = gltf.animations.find(a=>a.name===clipName);
console.log('clips', gltf.animations.map(a=>a.name+':'+a.duration));
const mixer = new THREE.AnimationMixer(gltf.scene);
mixer.clipAction(clip).play();
const skinned=[]; gltf.scene.traverse(o=>{ if(o.isSkinnedMesh) skinned.push(o)});
console.log('skinned meshes', skinned.length, 'bones', skinned[0].skeleton.bones.length);
mixer.setTime(parseFloat(time)); gltf.scene.updateMatrixWorld(true);
const m = skinned[0]; m.skeleton.update();
const pos = m.geometry.attributes.position;
const v = new THREE.Vector3();
const sampled = [];
for (let i=0;i<pos.count;i+=Math.floor(pos.count/2000)) { m.getVertexPosition(i,v); sampled.push([i,v.x,v.y,v.z]); }
if (!expectedPath) { console.log(JSON.stringify(sampled)); process.exit(0); }
const expected = JSON.parse(fs.readFileSync(expectedPath));
let worst = 0, moved = 0;
for (const [i,x,y,z] of expected) {
  const got = sampled.find(s=>s[0]===i);
  worst = Math.max(worst, Math.abs(got[1]-x), Math.abs(got[2]-y), Math.abs(got[3]-z));
  m.geometry.attributes.position.getX(i);
  moved = Math.max(moved, Math.abs(got[1]-pos.getX(i)), Math.abs(got[2]-pos.getY(i)), Math.abs(got[3]-pos.getZ(i)));
}
console.log('compared', expected.length, 'vertices at', clipName, time, 's');
console.log('max difference three.js vs gcad_mesh', worst);
console.log('max displacement from rest', moved);
process.exit(worst < 1e-5 ? 0 : 1);
