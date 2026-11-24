//! Where the time of reading one document goes. Run with
//! `NCRS_AI_MODELS_DIR=<folder> cargo test --release -p ncrs-ai bench -- --ignored --nocapture`;
//! `NCRS_BENCH_THREADS` (default 2) and `NCRS_BENCH_PRIORITY`
//! (`background` by default, `utility`, `user`) follow the app's gentle load.

use std::time::{Duration, Instant};

use ncrs_index::Field;
use ncrs_priority::ThreadPriority;

use super::tests::{models_dir, request};
use super::*;

const INVOICE: &str = "Stadtwerke Musterstadt GmbH\nHauptstraße 12, 12345 Musterstadt\n\n\
Rechnung Nr. 2024-04711\nRechnungsdatum: 05.03.2024\nKundennummer: 88 12 004\n\n\
Herrn Max Mustermann\nBeispielweg 7\n12345 Musterstadt\n\n\
Abrechnung Strom und Gas für den Zeitraum 01.01.2023 bis 31.12.2023\n\
Pos. Beschreibung Menge Einzelpreis Gesamt\n\
1 Grundpreis Strom 12 Monate 11,90 EUR 142,80 EUR\n\
2 Arbeitspreis Strom 2.850 kWh 0,3190 EUR 909,15 EUR\n\
3 Grundpreis Gas 12 Monate 9,50 EUR 114,00 EUR\n\
4 Arbeitspreis Gas 11.200 kWh 0,0980 EUR 1.097,60 EUR\n\
Nettobetrag 2.263,55 EUR\nUmsatzsteuer 19 % 430,07 EUR\nRechnungsbetrag 2.693,62 EUR\n\n\
Bereits gezahlte Abschläge 2.400,00 EUR\nNachzahlung 293,62 EUR\n\n\
Bitte überweisen Sie den Betrag bis zum 19.03.2024 auf das Konto\n\
IBAN DE89 3704 0044 0532 0130 00, BIC COBADEFFXXX.\n\
Vielen Dank für Ihr Vertrauen. Ihre Stadtwerke Musterstadt GmbH. \
Geschäftsführer Dr. Anna Beispiel, Amtsgericht Musterstadt HRB 1234, USt-IdNr. DE123456789.";

const STATEMENT: &str = "Sparkasse Musterstadt\nKontoauszug Nr. 3/2024\nKonto IBAN DE89 3704 0044 0532 0130 00\n\
Max Mustermann, Beispielweg 7, 12345 Musterstadt\nZeitraum 01.03.2024 bis 31.03.2024\n\n\
Alter Kontostand 4.120,33 EUR\n\
01.03. Miete Wohnung Beispielweg 7 -980,00\n\
04.03. Gehalt Muster Maschinenbau AG +3.250,00\n\
05.03. Stadtwerke Musterstadt Abschlag -200,00\n\
08.03. REWE Markt Musterstadt -54,12\n\
11.03. Telekom Deutschland GmbH -39,95\n\
15.03. Barauszahlung Geldautomat -100,00\n\
18.03. Versicherung Allianz Haftpflicht -12,40\n\
22.03. Amazon Payments Europe -67,89\n\
27.03. Zahlung Finanzamt Musterstadt -420,00\n\
29.03. Zinsen Guthaben +1,02\n\n\
Neuer Kontostand 5.496,99 EUR\nFreistellungsauftrag nicht erteilt. Einlagensicherung nach gesetzlicher Vorgabe.";

const LETTER: &str = "Finanzamt Musterstadt\nSteuernummer 123/456/78901\nMusterstadt, den 14. April 2024\n\n\
Herrn Max Mustermann\nBeispielweg 7\n12345 Musterstadt\n\n\
Einkommensteuerbescheid für 2022\n\n\
Sehr geehrter Herr Mustermann,\n\n\
auf Grund Ihrer Steuererklärung wurde die Einkommensteuer für das Jahr 2022 festgesetzt. \
Die Festsetzung führt zu einer Erstattung von 812,50 EUR, die in den nächsten Tagen auf Ihr \
Konto überwiesen wird. Gegen diesen Bescheid können Sie innerhalb eines Monats nach \
Bekanntgabe Einspruch einlegen. Der Einspruch ist schriftlich einzureichen oder zur Niederschrift zu erklären.\n\n\
Mit freundlichen Grüßen\nIm Auftrag\nSchmidt";

const OPEN: [Field; 4] = [Field::Sender, Field::Kind, Field::Date, Field::Amount];

struct Run {
    prompt_tokens: usize,
    prompt: Duration,
    reply_tokens: usize,
    sampling: Duration,
    decoding: Duration,
    reply: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sampling {
    /// The grammar sees the whole vocabulary for every token.
    GrammarThenGreedy,
    ReplySampler,
}

fn priority_from_env() -> ThreadPriority {
    match std::env::var("NCRS_BENCH_PRIORITY").as_deref() {
        Ok("utility") => ThreadPriority::Utility,
        Ok("user") => ThreadPriority::UserInitiated,
        _ => ThreadPriority::Background,
    }
}

fn run_once(model: &LocalModel, text: &str, threads: NonZeroU32, sampling: Sampling) -> Run {
    let request = request(text, &OPEN);
    let vocab = model.model.vocab();
    let prompt = prompt_tokens(&vocab, &request, 4096 - MAX_NEW_TOKENS).unwrap();
    let mut context = model
        .new_context(model.context_tokens, 512, threads)
        .unwrap();
    let mut batch = LlamaBatch::new(512, 1);

    let started = Instant::now();
    model
        .decode_tokens(&mut context, &mut batch, &prompt, 0, 512)
        .unwrap();
    let prompt_time = started.elapsed();

    let grammar = LlamaSampler::grammar(
        &model.model,
        &reply_grammar(&request.open_fields, request.judge_document),
        "root",
    )
    .unwrap();
    let mut chain = None;
    let mut replying = None;
    match sampling {
        Sampling::GrammarThenGreedy => {
            chain = Some(LlamaSampler::chain_simple([
                grammar,
                LlamaSampler::greedy(),
            ]));
        }
        Sampling::ReplySampler => replying = Some(ReplySampler::new(grammar)),
    }
    let (mut sampling_time, mut decoding_time) = (Duration::ZERO, Duration::ZERO);
    let mut reply = Vec::new();
    for _ in 0..MAX_NEW_TOKENS {
        let started = Instant::now();
        let index = batch.n_tokens() - 1;
        let token = match (&mut chain, &mut replying) {
            (Some(chain), _) => chain.sample(&context, index),
            (_, Some(replying)) => replying.next(&context, index),
            _ => unreachable!(),
        };
        sampling_time += started.elapsed();
        if vocab.is_eog(token) {
            break;
        }
        let position = i32::try_from(prompt.len() + reply.len()).unwrap();
        reply.push(token);
        batch.clear();
        batch.add(token, position, &[0], true).unwrap();
        let started = Instant::now();
        context.decode(&mut batch).unwrap();
        decoding_time += started.elapsed();
    }
    Run {
        prompt_tokens: prompt.len(),
        prompt: prompt_time,
        reply_tokens: reply.len(),
        sampling: sampling_time,
        decoding: decoding_time,
        reply: String::from_utf8_lossy(&vocab.detokenize(&reply, false, false)).into_owned(),
    }
}

#[test]
#[ignore = "loads the real local model from NCRS_AI_MODELS_DIR"]
fn bench_where_the_time_of_a_document_goes() {
    let threads = NonZeroU32::new(
        std::env::var("NCRS_BENCH_THREADS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(2),
    )
    .unwrap();
    let priority = priority_from_env();
    ncrs_priority::set_current_thread_priority(priority);
    let model = LocalModel::load(
        LocalModelConfig {
            path: models_dir().join(crate::model_file::LOCAL_MODEL.name),
            threads,
            context_tokens: NonZeroU32::new(4096).unwrap(),
            stop: Default::default(),
        },
        &crate::model_file::LOCAL_MODEL,
    )
    .unwrap();
    let layers = gpu_attempt().map(|(params, _)| params.n_gpu_layers());
    println!(
        "compute {:?}, n_gpu_layers asked {layers:?}, model layers {}, threads {threads}, priority {priority:?}",
        model.compute(),
        model.model.n_layer()
    );
    let _warm = run_once(&model, "Rechnung", threads, Sampling::ReplySampler);
    for (name, text) in [
        ("invoice", INVOICE),
        ("statement", STATEMENT),
        ("letter", LETTER),
    ] {
        for sampling in [Sampling::GrammarThenGreedy, Sampling::ReplySampler] {
            let run = run_once(&model, text, threads, sampling);
            let parsed = parse_reply(&run.reply, &request(text, &OPEN));
            println!(
                "{name:9} {sampling:?}: prompt {} tok {:.2}s | reply {} tok, decode {:.2}s, sampler {:.2}s | {:?}",
                run.prompt_tokens,
                run.prompt.as_secs_f32(),
                run.reply_tokens,
                run.decoding.as_secs_f32(),
                run.sampling.as_secs_f32(),
                parsed.map(|extraction| extraction.fields),
            );
        }
    }
}
