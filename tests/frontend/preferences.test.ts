import test from 'node:test';import assert from 'node:assert/strict';
import { defaultInputs,inputAction,inputError,latestStyles,defaultContext,contextPages } from '../../src/lib/preferences.ts';
test('current page defaults on; disabling the fallback preserves deliberate selections and region neighbors',()=>{
 const settings=defaultContext();assert.equal(settings.automaticCurrentPage,true);
 assert.deepEqual(contextPages(5,[],undefined,10,settings),[5]);
 settings.automaticCurrentPage=false;settings.beforePages=1;settings.afterPages=1;
 assert.deepEqual(contextPages(5,[],undefined,10,settings),[]);
 assert.deepEqual(contextPages(5,[2],undefined,10,settings),[2,1,3]);
 assert.deepEqual(contextPages(5,[],8,10,settings),[8,7,9]);
 const {automaticCurrentPage,...old}=settings;assert.deepEqual(contextPages(5,[],undefined,10,old),[5,4,6]);
});
test('application mappings reject conflicts and never capture input, composition or ordinary shortcuts',()=>{
 const settings=defaultInputs(),key={key:'!',code:'Digit1',ctrlKey:true,shiftKey:true,altKey:false,metaKey:false,isComposing:false};
 assert.equal(inputAction(key,settings,false),'region');assert.equal(inputAction(key,settings,true),null);assert.equal(inputAction({...key,isComposing:true},settings,false),null);assert.equal(inputAction({...key,keyCode:229},settings,false),null);assert.equal(inputAction({...key,ctrlKey:false},settings,false),null);assert.equal(inputAction({...key,code:'KeyS',key:'s'},settings,false),null);
 assert.equal(inputError(settings),'');const bad=structuredClone(settings);bad.bindings.quick=bad.bindings.region;assert.match(inputError(bad),/重复/);bad.bindings.quick='Ctrl+S';assert.match(inputError(bad),/保留/);
});
test('Preset search uses latest names and aliases and does not fabricate default/private styles',()=>{
 const style={id:'test',version:1,name:'简明',aliases:['短'],guidelines:'',avoid:'',enabled:true};
 assert.deepEqual(latestStyles({schemaVersion:1,selected:null,revisions:[]}),[]);
 const styles={schemaVersion:1,selected:null,revisions:[style,{...style,version:2,name:'新名称',aliases:['清楚'],enabled:false}]};
 assert.equal(latestStyles(styles,'短').length,0);assert.equal(latestStyles(styles,'清楚')[0].version,2);assert.equal(latestStyles(styles)[0].enabled,false);
});
test('multiple names normalize for search and recent choices remain first without changing revisions',()=>{
 const a={id:'a',version:1,name:'合成角色甲',aliases:['甲同学','Example A','测试学校'],guidelines:'',avoid:'',enabled:true};
 const b={...a,id:'b',name:'合成角色乙',aliases:['乙同学']},c={...a,id:'c',name:'合成角色丙',aliases:['丙同学']};
 const styles={schemaVersion:1,selected:null,revisions:[a,b,c],recent:['c','b']};
 assert.deepEqual(latestStyles(styles).map(s=>s.id),['c','b','a']);
 for(const alias of ['甲同学','测试学校','ｅｘａｍｐｌｅ Ａ'])assert.equal(latestStyles(styles,alias)[0].id,'a');
 assert.equal(styles.selected,null);assert.deepEqual(styles.revisions,[a,b,c]);
 assert.equal(latestStyles({...styles,recent:['not-existing']},'乙')[0].id,'b');
});
