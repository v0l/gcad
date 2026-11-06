import fs from 'fs'; import * as THREE from 'three';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';
const buf = fs.readFileSync(process.argv[2]);
const ab = buf.buffer.slice(buf.byteOffset, buf.byteOffset+buf.byteLength);
const gltf = await new Promise((res,rej)=>new GLTFLoader().parse(ab,'',res,rej));
const clip = gltf.animations.find(a=>a.name==='walk');
console.log('clips', gltf.animations.map(a=>a.name+':'+a.duration));
const mixer = new THREE.AnimationMixer(gltf.scene);
mixer.clipAction(clip).play();
const skinned=[]; gltf.scene.traverse(o=>{ if(o.isSkinnedMesh) skinned.push(o)});
console.log('skinned meshes', skinned.length, 'bones', skinned[0].skeleton.bones.length);
const t = parseFloat(process.argv[3]);
mixer.setTime(t); gltf.scene.updateMatrixWorld(true);
const v = new THREE.Vector3(); let out=[];
const m = skinned[0]; m.skeleton.update();
const pos = m.geometry.attributes.position;
for (let i=0;i<pos.count;i+=Math.floor(pos.count/2000)) { m.getVertexPosition(i,v); out.push([i,v.x,v.y,v.z]); }
fs.writeFileSync(process.argv[4], JSON.stringify(out));
