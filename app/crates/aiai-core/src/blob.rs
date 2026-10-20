//! Armazenamento de conteúdo por hash (BLAKE3). Tiles raster, máscaras, imagens importadas e áudio vivem aqui;
//! o documento guarda só o hash. Inserir um blob não é um comando (não tem undo): é idempotente e a coleta de lixo
//! só remove o que nem o documento nem o histórico referenciam.

use crate::error::{CoreError, CoreResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct BlobHash(pub String);

impl BlobHash {
    pub fn of(bytes: &[u8]) -> Self {
        BlobHash(blake3::hash(bytes).to_hex().to_string())
    }

    pub fn is_well_formed(&self) -> bool {
        self.0.len() == 64 && self.0.bytes().all(|b| b.is_ascii_hexdigit())
    }
}

impl std::fmt::Display for BlobHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Default, Clone)]
pub struct BlobStore {
    blobs: HashMap<BlobHash, Arc<Vec<u8>>>,
    /// Blobs ainda não gravados em disco (para salvar incremental).
    dirty: std::collections::HashSet<BlobHash>,
    /// Relógio de comandos (a sessão avança a cada comando aplicado) e quando cada blob entrou: a coleta só leva
    /// órfãos com pelo menos [`GC_MIN_AGE`] comandos de idade (um comando planejado e ainda não aplicado já pôs os
    /// tiles aqui; a coleta automática no meio de uma sequência não pode levá-los).
    now: u64,
    born: HashMap<BlobHash, u64>,
    /// Blobs que o documento cita mas que não puderam ser lidos ao abrir (arquivo ausente, truncado ou com hash
    /// diferente). A referência continua no documento (nada é apagado): o render desenha sem eles e avisa, os comandos
    /// aceitam movê-los (desfazer/refazer) e o Salvar não exige o conteúdo deles.
    missing: std::collections::BTreeSet<BlobHash>,
    /// Blobs conferidos (hash lido) ou gravados por nós na pasta atual do projeto: o Salvar não precisa relê-los.
    on_disk: std::collections::HashSet<BlobHash>,
}

/// Idade mínima (em comandos aplicados) de um blob órfão para a coleta levar.
pub const GC_MIN_AGE: u64 = 32;

impl BlobStore {
    pub fn put(&mut self, bytes: Vec<u8>) -> BlobHash {
        let h = BlobHash::of(&bytes);
        if !self.blobs.contains_key(&h) {
            // conteúdo refeito de um blob danificado: volta a existir (o disco é regravado no próximo diário/salvar)
            self.missing.remove(&h);
            self.blobs.insert(h.clone(), Arc::new(bytes));
            self.dirty.insert(h.clone());
            self.born.insert(h.clone(), self.now);
        }
        h
    }

    /// Insere um blob lido do disco (já persistido).
    pub fn insert_clean(&mut self, hash: BlobHash, bytes: Vec<u8>) -> CoreResult<()> {
        if BlobHash::of(&bytes) != hash {
            return Err(CoreError::invalid("blob", format!("conteúdo não confere com o hash {hash}")));
        }
        self.missing.remove(&hash);
        self.blobs.insert(hash, Arc::new(bytes));
        Ok(())
    }

    /// Marca um blob citado pelo documento cujo conteúdo não pôde ser lido (ver `missing`).
    pub fn mark_missing(&mut self, h: BlobHash) {
        if !self.blobs.contains_key(&h) {
            self.missing.insert(h);
        }
    }

    pub fn is_missing(&self, h: &BlobHash) -> bool {
        self.missing.contains(h)
    }

    /// Blobs danificados/ausentes conhecidos desde a abertura.
    pub fn missing(&self) -> &std::collections::BTreeSet<BlobHash> {
        &self.missing
    }

    /// O conteúdo está aqui ou é um blob danificado já conhecido (referência herdada do documento): os comandos aceitam
    /// citar os dois; um hash desconhecido continua sendo erro.
    pub fn known(&self, h: &BlobHash) -> bool {
        self.blobs.contains_key(h) || self.missing.contains(h)
    }

    pub fn mark_on_disk(&mut self, h: BlobHash) {
        self.on_disk.insert(h);
    }

    pub fn is_on_disk(&self, h: &BlobHash) -> bool {
        self.on_disk.contains(h)
    }

    /// A pasta do projeto mudou (Salvar como): nada está conferido na pasta nova.
    pub fn clear_on_disk(&mut self) {
        self.on_disk.clear();
    }

    pub fn get(&self, h: &BlobHash) -> Option<Arc<Vec<u8>>> {
        self.blobs.get(h).cloned()
    }

    pub fn contains(&self, h: &BlobHash) -> bool {
        self.blobs.contains_key(h)
    }

    pub fn len(&self) -> usize {
        self.blobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }

    pub fn total_bytes(&self) -> usize {
        self.blobs.values().map(|b| b.len()).sum()
    }

    pub fn take_dirty(&mut self) -> Vec<BlobHash> {
        self.dirty.drain().collect()
    }

    pub fn mark_dirty(&mut self, h: BlobHash) {
        self.dirty.insert(h);
    }

    /// Avança o relógio de comandos (chamado pela sessão a cada comando aplicado).
    pub fn tick(&mut self) {
        self.now += 1;
    }

    /// Remove o que não está em `keep`, menos os blobs mais novos que [`GC_MIN_AGE`] comandos.
    pub fn retain(&mut self, keep: &std::collections::BTreeSet<BlobHash>) -> usize {
        let before = self.blobs.len();
        let now = self.now;
        let young = |born: &HashMap<BlobHash, u64>, h: &BlobHash| born.get(h).is_some_and(|&b| now.saturating_sub(b) < GC_MIN_AGE);
        let born = std::mem::take(&mut self.born);
        self.blobs.retain(|h, _| keep.contains(h) || young(&born, h));
        self.dirty.retain(|h| keep.contains(h) || young(&born, h));
        self.born = born.into_iter().filter(|(h, _)| self.blobs.contains_key(h)).collect();
        let blobs = &self.blobs;
        self.on_disk.retain(|h| blobs.contains_key(h));
        before - self.blobs.len()
    }
}
