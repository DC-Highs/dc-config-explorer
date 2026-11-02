# DC Config Explorer

Aplicativo desktop para explorar o `game_config` do [Dragon City](https://dragoncitygame.com/),
feito com [egui](https://www.egui.rs/) e [fastframe](https://fastframe.dev/).
Fan-made, sem afiliação com a SocialPoint.

## O que faz

| Tela | Crate | O que mostra |
| --- | --- | --- |
| Visão geral | `dc-config` | seções do config, tipo, número de entradas, quais têm mapper |
| Seção → **Bruto** | `dc-config` | o JSON original da seção |
| Seção → **Mapeado** | `dc-config-mapper` | a seção reestruturada, validada e traduzida (chests, items, skills, heroic_races, maze/tower/grid/fog/runner islands) |
| Estatísticas | `dc-statistics` (+ `dc-core`) | dragões por raridade/elemento/categoria, ilhas ativas/futuras/passadas, itens por grupo, totais |
| Localização | `dc-localization` | busca nas ~38 mil chaves (sem distinção de acentos), exporta e compara versões (novas/editadas/removidas) |

O explorer de JSON:

* lista virtualizada (aguenta seções com milhares de entradas) ou **tabela** para arrays de objetos;
* filtro por chave ou conteúdo, breadcrumb, teclado (↑ ↓, Enter abre, Backspace sobe);
* inspetor com o JSON do item selecionado, **traduções** das chaves `tid_*`/`*_key` no idioma escolhido,
  copiar caminho (`items[12].name`) / JSON e exportar para arquivo.

Fontes do config: arquivo local (também por arrastar-e-soltar ou `dc-config-explorer config.json`),
URL pública ou o endpoint autenticado (URL, user id, auth token). Aceita o formato
`{ "game_data": { "config": … } }` e também arquivos só com o objeto de seções.

## Uso de fastframe

* `fastframe-fonts` + `fastframe-text`: Inter em todos os pesos e renderização de texto como a do sistema;
* `fastframe-dirs`: pasta de configuração da app (com suporte a cópia portátil no Windows);
* `fastframe-file`: as configurações são gravadas de forma atômica numa thread própria.
  O auth token só é salvo se "Lembrar token" estiver marcado, num arquivo privado separado.

## Compilar

Requer **Rust 1.98+** (exigência do fastframe).

```sh
cargo run --release
cargo run --release -- caminho/para/config.json
```

## Testes

```sh
cargo test
# com um config real: roda todos os mappers e o dc-statistics sobre ele
DC_CONFIG_FIXTURE=/caminho/config.json cargo test --release -- --nocapture
# baixa a localização e testa export/compare (rede)
DC_NETWORK_TESTS=1 cargo test --release -- --nocapture
```
