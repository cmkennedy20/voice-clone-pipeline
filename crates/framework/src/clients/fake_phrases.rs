//! Deterministic digit-free multilingual spoken fixtures for `FakeMandarinTranslator` and `FakeSpeechRecognizer`.

use domain::ports::TargetLanguage;

/// Sample translation & clean reverse-`ASR` tuple for a [`TargetLanguage`].
#[derive(Debug, Clone, Copy)]
pub struct FakeLanguageSample {
    pub spoken_text: &'static str,
    pub phonetic_guide: &'static str,
    pub english_back_gloss: &'static str,
    pub clean_back_transcribed: &'static str,
    pub mumbled_back_transcribed: &'static str,
}

#[must_use]
pub const fn sample_for_language(lang: TargetLanguage) -> FakeLanguageSample {
    match lang {
        TargetLanguage::Mandarin => FakeLanguageSample {
            spoken_text: "今天下午三点半我们开会。",
            phonetic_guide: "jīn tiān xià wǔ sān diǎn bàn wǒ men kāi huì.",
            english_back_gloss: "We have a meeting at three thirty this afternoon.",
            clean_back_transcribed: "今天下午三点半我们开会",
            mumbled_back_transcribed: "完全不同的胡言乱语测试文本",
        },
        TargetLanguage::Spanish => FakeLanguageSample {
            spoken_text: "Hola equipo, tenemos una reunión a las tres y media de la tarde.",
            phonetic_guide: "OH-lah eh-KEE-poh, teh-NEH-mohs OO-nah reh-oo-NYOHN ah lahs trehs ee MEH-dyah deh lah TAR-deh.",
            english_back_gloss: "Hello team, we have a meeting at three thirty in the afternoon.",
            clean_back_transcribed: "hola equipo tenemos una reunión a las tres y media de la tarde",
            mumbled_back_transcribed: "texto alucinado completamente incomprensible sin sentido",
        },
        TargetLanguage::French => FakeLanguageSample {
            spoken_text: "Bonjour l'équipe, nous avons une réunion cet après-midi.",
            phonetic_guide: "bohn-ZHOOR leh-KEEP, noo zah-VOHN oon reh-oo-NYOHN set ah-preh-mee-DEE.",
            english_back_gloss: "Hello team, we have a meeting this afternoon.",
            clean_back_transcribed: "bonjour l'équipe nous avons une réunion cet après-midi",
            mumbled_back_transcribed: "texte complètement incohérent et brouillé sans aucun sens",
        },
        TargetLanguage::German => FakeLanguageSample {
            spoken_text: "Hallo zusammen, wir haben heute Nachmittag um halb vier ein Treffen.",
            phonetic_guide: "HAH-loh tsoo-ZAH-men, veer HAH-ben HOY-teh NAKH-mi-tahk oom halp feer ayn TREH-fen.",
            english_back_gloss: "Hello everyone, we have a meeting at half past three this afternoon.",
            clean_back_transcribed: "hallo zusammen wir haben heute nachmittag um halb vier ein treffen",
            mumbled_back_transcribed: "völlig unverständlicher halluzinierter testtext ohne sinn",
        },
        TargetLanguage::Italian => FakeLanguageSample {
            spoken_text: "Ciao squadra, abbiamo una riunione oggi pomeriggio alle tre e mezza.",
            phonetic_guide: "CHOW SKWAH-drah, ahb-BYAH-moh OO-nah ree-oo-NYOH-neh OHD-jee poh-meh-REED-joh.",
            english_back_gloss: "Hi team, we have a meeting this afternoon at three thirty.",
            clean_back_transcribed: "ciao squadra abbiamo una riunione oggi pomeriggio alle tre e mezza",
            mumbled_back_transcribed: "testo completamente diverso incomprensibile e confuso",
        },
        TargetLanguage::Portuguese => FakeLanguageSample {
            spoken_text: "Olá equipe, temos uma reunião hoje à tarde às três e meia.",
            phonetic_guide: "oh-LAH eh-KEE-pee, TEH-moos OO-mah heh-oo-NYOWN OH-zhee ah TAR-jee.",
            english_back_gloss: "Hello team, we have a meeting this afternoon at three thirty.",
            clean_back_transcribed: "olá equipe temos uma reunião hoje à tarde às três e meia",
            mumbled_back_transcribed: "texto completamente diferente incompreensível sem sentido",
        },
        TargetLanguage::Japanese => FakeLanguageSample {
            spoken_text: "今日の午後三時半に会議があります。",
            phonetic_guide: "kyō no gogo sanji han ni kaigi ga arimasu.",
            english_back_gloss: "There is a meeting at three thirty this afternoon.",
            clean_back_transcribed: "今日の午後三時半に会議があります",
            mumbled_back_transcribed: "全く異なる意味不明なテスト音声認識結果",
        },
        TargetLanguage::Korean => FakeLanguageSample {
            spoken_text: "오늘 오후 세 시 반에 팀 회의가 있습니다.",
            phonetic_guide: "oneul ohu se si bane tim hoeuiga itseumnida.",
            english_back_gloss: "There is a team meeting at three thirty this afternoon.",
            clean_back_transcribed: "오늘 오후 세 시 반에 팀 회의가 있습니다",
            mumbled_back_transcribed: "완전히 다른 알아들을 수 없는 테스트 문장입니다",
        },
        TargetLanguage::Russian => FakeLanguageSample {
            spoken_text: "Привет команда, сегодня в три тридцать дня у нас встреча.",
            phonetic_guide: "pree-VYET kah-MAHN-dah, see-VOHD-nya v tree TREED-tsat dnya oo nahs VSTREH-cha.",
            english_back_gloss: "Hello team, today at three thirty in the afternoon we have a meeting.",
            clean_back_transcribed: "привет команда сегодня в три тридцать дня у нас встреча",
            mumbled_back_transcribed: "совершенно другой неразборчивый бессмысленный текст",
        },
        TargetLanguage::Arabic => FakeLanguageSample {
            spoken_text: "مرحبا يا فريق لدينا اجتماع في الساعة الثالثة والنصف بعد الظهر",
            phonetic_guide: "marhaban ya fariq ladayna ijtima fi al-sa'a al-thalitha wa al-nisf ba'd al-zuhr",
            english_back_gloss: "Hello team, we have a meeting at three thirty in the afternoon.",
            clean_back_transcribed: "مرحبا يا فريق لدينا اجتماع في الساعة الثالثة والنصف بعد الظهر",
            mumbled_back_transcribed: "نص مختلف تماما غير مفهوم ولا معنى له على الإطلاق",
        },
        TargetLanguage::Hindi => FakeLanguageSample {
            spoken_text: "नमस्ते टीम आज दोपहर साढ़े तीन बजे हमारी बैठक है।",
            phonetic_guide: "namaste teem aaj dopahar saadhe teen baje hamaaree baithak hai.",
            english_back_gloss: "Hello team, our meeting is at three thirty this afternoon.",
            clean_back_transcribed: "नमस्ते टीम आज दोपहर साढ़े तीन बजे हमारी बैठक है",
            mumbled_back_transcribed: "बिल्कुल अलग अस्पष्ट और निरर्थक परीक्षण वाक्य",
        },
        TargetLanguage::English => FakeLanguageSample {
            spoken_text: "Hello team, we have a meeting at three thirty this afternoon.",
            phonetic_guide: "heh-LOH teem, wee hav uh MEE-ting at three THUR-tee this af-ter-NOON.",
            english_back_gloss: "Hello team, we have a meeting at three thirty this afternoon.",
            clean_back_transcribed: "hello team we have a meeting at three thirty this afternoon",
            mumbled_back_transcribed: "completely different unintelligible hallucinated gibberish phrase",
        },
    }
}
