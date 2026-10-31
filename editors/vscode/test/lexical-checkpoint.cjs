const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');
exports.run = async function(root) {
  const uri=vscode.Uri.file(path.join(root,'recovery.tellur'));
  const source=await fs.readFile(path.join(__dirname,'../../../playground/recovery.tellur'),'utf8');
  await fs.writeFile(uri.fsPath,source);
  const document=await vscode.workspace.openTextDocument(uri);
  await vscode.window.showTextDocument(document);
  await vscode.extensions.getExtension('tellur-language-local.tellur-language').activate();
  const diagnostics=()=>vscode.languages.getDiagnostics(uri).filter(d=>d.source==='tellur');
  const code=d=>typeof d.code==='object'?d.code.value:d.code;
  async function waitFor(predicate,description) {
    const end=Date.now()+15000;
    while(Date.now()<end) {if(await predicate()) return; await new Promise(resolve=>setTimeout(resolve,50));}
    throw new Error(`Timed out: ${description}`);
  }
  async function replace(text) {
    const change=new vscode.WorkspaceEdit();
    change.replace(uri,new vscode.Range(document.positionAt(0),document.positionAt(document.getText().length)),text);
    assert.ok(await vscode.workspace.applyEdit(change));
  }
  await waitFor(async()=>{
    const hover=await vscode.commands.executeCommand('vscode.executeHoverProvider',uri,document.positionAt(source.indexOf('@fault')+2));
    return diagnostics().length===0 && hover?.some(h=>h.contents.some(c=>c.value.includes('ref fault_report')));
  },'structured @fault hover');
  await replace(source.replace('recover again requestor;','exit done;'));
  await waitFor(()=>diagnostics().some(d=>code(d)==='E_CONTINGENCY_EXIT'),'ordinary exit from contingency rejected');
  await replace(source);
  await waitFor(()=>diagnostics().length===0,'recovery repair');
  await replace(source.replace('contingency [context]','contingency[context]'));
  const edits=await vscode.commands.executeCommand('vscode.executeFormatDocumentProvider',uri,{tabSize:4,insertSpaces:true});
  assert.ok(edits?.length>0);
  await replace(source);
  await document.save();
  const result=await vscode.commands.executeCommand('tellur.buildAndRun',{arguments:['--json'],output:path.join(root,'native output','lexical-recovery')});
  assert.equal(result.code,0,result.stderr);
  const report=JSON.parse(result.stdout);
  assert.equal(report.outcome,'done');
  assert.deepEqual(report.events.filter(e=>e.kind==='emit').map(e=>e.message),['retired work','request failed','request context']);
  const stop='outcome program_result {@exit 0 done} control main() exits program_result {fault boundary {fault "startup";} contingency [] {terminate process 23;}}';
  await replace(stop);
  await document.save();
  const stopped=await vscode.commands.executeCommand('tellur.buildAndRun',{arguments:['--json'],output:path.join(root,'native output','lexical-termination')});
  assert.equal(stopped.code,23,stopped.stderr);
  const termination=JSON.parse(stopped.stdout);
  assert.equal(termination.termination,'process');
  assert.equal(termination.outcome,undefined);
  await replace(source);
  await document.save();
  console.log('TELLUR_LEXICAL_EDITOR_TESTS_PASSED: structured hover, forbidden return and repair, formatting, native recovery and process status without an outcome');
};
