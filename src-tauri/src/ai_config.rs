use serde::{Deserialize,Serialize};
use crate::{error::{AppError,Result},ai_network::{endpoint,failure}};
#[derive(Clone,Deserialize,Serialize,PartialEq)]#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct Model {pub provider:String,pub endpoint:String,pub model:String,pub vision:bool}
impl Model{pub(crate) fn validate(&self)->Result<()>{
 if !matches!(self.provider.as_str(),"mock"|"deepseek"|"openrouter"|"compatible")||self.model.is_empty()||self.model.len()>160||self.model.chars().any(|c|c.is_control()){return Err(failure("invalid_request"));}
 if self.provider!="mock"{let url=endpoint(&self.endpoint)?;if self.provider=="deepseek"&&url.as_str()!="https://api.deepseek.com/chat/completions"||self.provider=="openrouter"&&url.as_str()!="https://openrouter.ai/api/v1/chat/completions"{return Err(failure("endpoint"));}}Ok(())
}}
#[derive(Clone,Deserialize,Serialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct Config {pub schema_version:u32,pub main:Model,pub auxiliary:Option<Model>}
impl Default for Config {fn default()->Self{Self{schema_version:1,main:Model{provider:"mock".into(),endpoint:String::new(),model:"mock-vision".into(),vision:true},auxiliary:None}}}
impl Config {pub(crate) fn validate(&self)->Result<()>{if self.schema_version!=1{return Err(AppError::denied());}self.main.validate()?;if let Some(aux)=&self.auxiliary{aux.validate()?;}Ok(())}}
