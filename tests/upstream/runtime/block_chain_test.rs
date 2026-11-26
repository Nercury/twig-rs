//! TWIG/tests/BlockChainTest.php is not ported: twig-rs has no `Twig\BlockChain` (Twig 3.31's composition of the
//! blocks of several templates and chains, missing API). Its 44 methods (58 PHPUnit tests with the `yieldModes`
//! data sets; `use_yield` changes nothing in twig-rs) all construct a `BlockChain`:
//!
//! `testComposesCompleteTemplateLineagesInPrecedenceOrder`,
//! `testNestedBlocksUseTheComposedSetAndParentUsesTheRenderContextLineage`,
//! `testExplicitTemplateBlockCallsResolveOutsideTheChainNamespace`,
//! `testTheConstructorContextIsADefaultTheRenderContextCanOverride`,
//! `testIntrospectionAndRenderingAgreeOnTheSameContext`, `testRenderingThroughTheChainDoesNotChangeTheLoadedTemplate`,
//! `testStructuralContextIncludesEnvironmentGlobals`, `testTraitAliasesKeepTheirLocalParentLineage`,
//! `testRenderCapturesLegacyEchoingBlocks`, `testRenderRestoresOutputBuffersOnError`,
//! `testRenderingDisplayingAndStreamingAddGlobals`, `testRejectsInvalidBlockDefinitions`,
//! `testRejectsTemplatesThatAreNotStringsWrappersOrChains`, `testAChainedChainKeepsItsPositionInThePrecedenceOrder`,
//! `testNestedBlockCallsSeeTheBlocksOfAChainedChain`, `testAChainedChainIsComposedThroughItsWholeLineage`,
//! `testAChainedChainKeepsItsOwnDefaultContext`, `testAChainedChainWithADynamicParentIsResolvedAgainstTheRenderContext`,
//! `testRejectsChainsFromAnotherEnvironment`, `testRejectsWrappersFromAnotherEnvironment`,
//! `testRejectsWrappersThatHideATemplateFromAnotherEnvironment`, `testRejectsDynamicParentsFromAnotherEnvironment`,
//! `testDynamicParentSecurityIsCheckedWhenTheLineageIsResolved`, `testDefiningTemplateSecurityIsCheckedDuringRendering`,
//! `testSandboxPolicyChangesAreObservedAfterConstruction`, `testSandboxPolicyChangesAreCheckedOnIntermediateParents`,
//! `testImportedMacroNamespacesObserveSandboxPolicyChanges`, `testProfilerKeepsTheDefiningTemplateAndBlockAttribution`,
//! `testMacrosStayOwnedByTheirDefiningTemplate`, `testInheritedMacroLookupUsesTheRenderContextLineage`,
//! `testSelfMacroImportsUseTheRenderContextLineage`, `testFromSelfImportsResolveThroughTheLineage`,
//! `testImportedMacroNamespacesKeepTheirOwnLineage`, `testModuleImportsFollowTheDefiningTemplateBodyState`,
//! `testSelfMacroImportsInMacroBodiesResolveThroughTheLineage`,
//! `testMacroBodiesCannotResolveADynamicParentJustLikeADirectRender`,
//! `testMacroBodiesObserveImportUpdatesAfterConstruction`, `testPreWarmedExternalMacroImportsAreNotReboundByChainOrder`,
//! `testChainsWithDifferentDynamicParentsCanBeStreamedInterleaved`, `testErrorsKeepTheDefiningSourceAndLine`,
//! `testUnknownBlockUsesTheFirstTemplateAsErrorContext`,
//! `testUnknownBlockUsesTheFirstTemplateOfAChainedChainAsErrorContext`, `testCircularInheritanceIsRejected`,
//! `testRequiresAtLeastOneTemplate`.
//!
//! The direct renders some of them make before using the chain (`$twig->render('theme', ...)`) are template behaviour
//! that the composition vectors of `tests/engine.rs` cover.
