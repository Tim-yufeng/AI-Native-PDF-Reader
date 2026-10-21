import { invoke } from '@tauri-apps/api/core';
export interface FolderWorkspace {id:string;name:string;}
export interface FolderEntry {id:string;name:string;directory:boolean;}
export interface FolderListing {entries:FolderEntry[];skippedLinks:number;}
// No path or AI permission crosses this boundary; nodes come from enumeration.
export const folderWorkspace={
  pick:()=>invoke<FolderWorkspace|null>('pick_workspace'),
  list:(workspaceId:string,nodeId:string)=>invoke<FolderListing>('list_workspace',{workspaceId,nodeId}),
  open:(workspaceId:string,nodeId:string)=>invoke<import('./desktop-source').DesktopDescriptor>('open_workspace_pdf',{workspaceId,nodeId}),
  close:(workspaceId:string)=>invoke<void>('close_workspace',{workspaceId}),
};
