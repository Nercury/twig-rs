use twig::extension::Apply;
use twig::environment::{ ExtendEnvironment };

pub struct TranslationExtension;

impl Apply for TranslationExtension {
    fn apply<E: ExtendEnvironment>(env: &mut E) {
        
    }
}
