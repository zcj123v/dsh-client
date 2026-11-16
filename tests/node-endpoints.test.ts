import { expect, it } from 'vitest'
import { createConnectionRecord } from '../src/connection-model'
import { ConnectionStore } from '../src/connection-store'

const id='170ce412-3cbb-4467-a35a-b49fce65e8ef'
const entries=[{ label:'LAN',mode:'ssh-forward' as const,sshProfile:'192.168.1.20',sshUser:'tester',sshConfig:'none' as const,remotePort:3080 },{label:'WG',mode:'ssh-forward' as const,sshProfile:'10.250.0.13',sshUser:'tester',sshConfig:'none' as const,remotePort:3080}]
it('round-trips ordered entries without leaking credentials or sharing mutable arrays',()=>{
 const values=new Map<string,string>();const storage={getItem:(k:string)=>values.get(k)??null,setItem:(k:string,v:string)=>{values.set(k,v)},removeItem:(k:string)=>{values.delete(k)}}
 const record=createConnectionRecord({name:'Air',mode:'direct',endpoints:entries.map(e=>({...e,token:'NEVER_SAVE'}))},()=>id)
 const store=new ConnectionStore(storage);store.save(record)
 const restored=new ConnectionStore(storage)
 expect(restored.list()[0]?.endpoints?.map(e=>e.label)).toEqual(['LAN','WG'])
 expect(values.get('dsh-client.connections.v1')).not.toContain('NEVER_SAVE')
 const copy=restored.list();copy[0]!.endpoints![0]!.label='mutated'
 expect(restored.list()[0]?.endpoints?.[0]?.label).toBe('LAN')
})
it('rejects a malicious later entry and excessive entry counts',()=>{
 expect(()=>createConnectionRecord({name:'A',mode:'direct',url:'https://example.test/',endpoints:[{label:'LAN',mode:'direct',url:'http://127.0.0.1:3080/'},{label:'bad',mode:'direct',url:'https://example.test/?token=SECRET'}]})).toThrow()
 expect(()=>createConnectionRecord({name:'A',mode:'direct',url:'https://example.test/',endpoints:Array(9).fill(entries[0])})).toThrow()
})
