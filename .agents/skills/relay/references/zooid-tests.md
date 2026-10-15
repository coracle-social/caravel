# zooid test inventory

Every test in zooid's suite, and the caravel test that covers the same behavior or why none does.

## api_test.go

- `TestAPIHandler_Authentication` — Pending: ported in P2–P4.
- `TestAPIHandler_CreateRelay` — Pending: ported in P2–P4.
- `TestAPIHandler_UpdateRelay` — Pending: ported in P2–P4.
- `TestAPIHandler_PatchRelay` — Pending: ported in P2–P4.
- `TestAPIHandler_DeleteRelay` — Pending: ported in P2–P4.
- `TestAPIHandler_ListRelayMembers` — Pending: ported in P2–P4.
- `TestAPIHandler_GetRelayBlob` — Pending: ported in P2–P4.
- `TestAPIHandler_ListModeration` — Pending: ported in P2–P4.
- `TestAPIHandler_ListModeration_Truncated` — Pending: ported in P2–P4.
- `TestCollectModerationItems_OrdersBySeenAt` — Pending: ported in P2–P4.
- `TestAPIHandler_ListModeration_RedirectInstance` — Pending: ported in P2–P4.
- `TestAPIHandler_ListModeration_Unmoderated` — Pending: ported in P2–P4.
- `TestAPIHandler_MethodNotAllowed` — Pending: ported in P2–P4.
- `TestAPIHandler_InvalidPath` — Pending: ported in P2–P4.
- `TestAPIHandler_ConfigValidation` — Pending: ported in P2–P4.
- `TestAPIHandler_RelayMemberWrites` — Pending: ported in P2–P4.
- `TestAPIHandler_RelayNotifications` — Pending: ported in P2–P4.
- `TestAPIHandler_RelayClaims` — Pending: ported in P2–P4.
- `TestNewAPIHandler` — Pending: ported in P2–P4.
- `TestAPIHandler_GetRelayUsage` — Pending: ported in P2–P4.
- `TestAPIHandler_LivekitWebhook` — Pending: ported in P2–P4.
- `TestAPIHandler_ImportExportRelay` — Pending: ported in P2–P4.

## blossom_test.go

- `TestBlossomStore_Enable_DeleteWithoutAuthHeaderDoesNotPanic` — Pending: ported in P3.
- `TestBlossomStore_Enable_DeleteWithMalformedAuthHeaderDoesNotPanic` — Pending: ported in P3.
- `TestBlossomStore_Enable_ServesUnrenderableBlobsAsDownloads` — Pending: ported in P3.
- `TestBlossomStore_Enable_ExposesRefusalReason` — Pending: ported in P3.
- `TestIsInlineBlobType` — Pending: ported in P3.
- `TestBlossomStore_StorageBytes` — Pending: ported in P3.

## config_test.go

- `TestConfig_IsOwner` — `crates/core/tests/relay_policy.rs` members_and_managers_may_write_and_strangers_may_not
- `TestConfig_IsSelf` — `crates/core/tests/relay_policy.rs` writing_needs_authentication_as_the_author
- `TestValidatePayments` — Pending: ported in P4.
- `TestValidateBlossomFileStorage` — Obsolete: storage is platform configuration, not per relay.
- `TestValidateMemberMethods` — Pending: ported in P2.
- `TestValidateAnonymousRead` — Obsolete: anonymous reads are a relay setting the API takes as a flag.
- `TestValidateRedirect` — Obsolete: there are no config files.
- `TestValidateRedirectFrom` — Obsolete: redirects are `host_redirect` rows written by the core.
- `TestLoadConfigFromPath_BlossomS3` — Obsolete: there are no config files.

## env_test.go

- `TestLoadEnvReadsDotenv` — Obsolete: the app reads `.env` with dotenvy.
- `TestLoadEnvWithoutDotenv` — Obsolete: the app reads `.env` with dotenvy.

## events_test.go

- `TestEventStore_SaveEvent` — `crates/core/tests/events.rs` a_saved_event_reads_back_exactly
- `TestEventStore_QueryEvents_Basic` — `crates/core/tests/events.rs` queries_match_kinds_authors_ids_tags_and_time
- `TestEventStore_QueryEvents_ByKind` — `crates/core/tests/events.rs` queries_match_kinds_authors_ids_tags_and_time
- `TestEventStore_QueryEvents_ByAuthor` — `crates/core/tests/events.rs` queries_match_kinds_authors_ids_tags_and_time
- `TestEventStore_QueryEvents_ByIDs` — `crates/core/tests/events.rs` queries_match_kinds_authors_ids_tags_and_time
- `TestEventStore_QueryEvents_ByTags` — `crates/core/tests/events.rs` queries_match_kinds_authors_ids_tags_and_time
- `TestEventStore_QueryEvents_TimeRange` — `crates/core/tests/events.rs` queries_match_kinds_authors_ids_tags_and_time
- `TestEventStore_QueryEvents_Limit` — `crates/core/tests/events.rs` limits_cap_results_and_ties_break_on_id
- `TestEventStore_QueryEvents_LimitZero` — `crates/core/tests/events.rs` limits_cap_results_and_ties_break_on_id
- `TestEventStore_QueryEvents_Search` — `crates/core/tests/events.rs` search_matches_words_in_sequence
- `TestEventStore_DeleteEvent` — `crates/core/tests/events.rs` a_deletion_removes_its_authors_events_and_refuses_them_after
- `TestEventStore_ReplaceEvent` — `crates/core/tests/events.rs` a_replaceable_event_keeps_one_newest_version_per_slot
- `TestEventStore_ReplaceEvent_Republish` — `crates/core/tests/events.rs` a_replaceable_event_keeps_one_newest_version_per_slot
- `TestEventStore_ReplaceEvent_OlderEvent` — `crates/core/tests/events.rs` a_replaceable_event_keeps_one_newest_version_per_slot
- `TestEventStore_CountEvents` — Obsolete: the relays answer COUNT with a refusal, as zooid did, so nothing counts.
- `TestEventStore_Close` — Obsolete: there is no per-relay store to close.
- `TestEventStore_GetOrCreateApplicationSpecificData` — Obsolete: relay state lives in tables, not app-data events.
- `TestEventStore_QueryEvents_MaxLimitCapsFilterWithNoLimit` — `crates/core/tests/events.rs` limits_cap_results_and_ties_break_on_id
- `TestEventStore_CountEvents_IgnoresFilterLimit` — Obsolete: nothing counts events.
- `TestEventStore_QueryEvents_SearchPhrase` — `crates/core/tests/events.rs` search_matches_words_in_sequence
- `TestEventStore_SaveEvent_ConcurrentDuplicate` — `crates/core/tests/events.rs` a_saved_event_is_a_duplicate_the_second_time
- `TestEventStore_QueryEvents_EmptyFilterFieldMatchesNothing` — `crates/core/tests/events.rs` an_absent_field_matches_everything_and_an_empty_one_nothing
- `TestEventStore_QueryEvents_NilFilterFieldsMatchEverything` — `crates/core/tests/events.rs` an_absent_field_matches_everything_and_an_empty_one_nothing

## groups_test.go

- `TestGetGroupIDFromEvent` — Pending: ported in P2.
- `TestGroupStore_UpdatePins` — Pending: ported in P2.
- `TestGroupStore_AddMember_IsIdempotent` — Pending: ported in P2.
- `TestGroupStore_RemoveMember_SkipsNonMember` — Pending: ported in P2.
- `TestGroupStore_CheckWrite_PutPins` — Pending: ported in P2.
- `TestGroupStore_DeleteGroup_RemovesPins` — Pending: ported in P2.
- `TestGroupStore_DeleteEvents` — Pending: ported in P2.
- `TestGroupStore_DeleteEvents_SkipsAnotherGroup` — Pending: ported in P2.
- `TestGroupStore_DeleteEvents_SkipsGroupState` — Pending: ported in P2.
- `TestGroupStore_CheckWrite_DeleteEvent` — Pending: ported in P2.
- `TestGroupStore_IsInvited` — Pending: ported in P2.
- `TestGroupStore_CheckWrite_HiddenGroupJoinWithInvite` — Pending: ported in P2.
- `TestGroupStore_CacheInvalidation` — Pending: ported in P2.

## instance_test.go

- `TestMakeInstance_Payments` — Pending: ported in P4.
- `TestMakeInstance_NoPayments` — Pending: ported in P4.
- `TestMakeInstance_Redirect` — Obsolete: redirect-only configs became `host_redirect` rows, covered with the hosts in `crates/relay/tests/protocol.rs`.
- `TestInstance_AllowRecipientEvent` — `crates/core/tests/relay_policy.rs` a_gift_wrap_or_zap_for_a_member_needs_no_authentication
- `TestInstance_PreventBroadcast_PrivateGroup` — Pending: ported in P2.
- `TestInstance_OnRequest_Anonymous` — `crates/core/tests/relay_policy.rs` reading_needs_authentication_unless_anonymous_reads_are_on
- `TestInstance_QueryStored_Anonymous` — `crates/relay/tests/protocol.rs` an_anonymous_relay_serves_readers_who_never_authenticate
- `TestInstance_IsInternalEvent` — `crates/core/tests/relay_policy.rs` kinds_the_relay_writes_itself_are_refused
- `TestInstance_OnEventSaved_ClosedGroupJoinWithInvite` — Pending: ported in P2.
- `TestInstance_CachesSeeClientWrites` — Pending: ported in P2.
- `TestInstance_BanEventInvalidatesCaches` — Pending: ported in P2.
- `TestInstance_OnEvent_CreatedAtUpperLimit` — `crates/core/tests/relay_policy.rs` events_dated_too_far_ahead_are_refused
- `TestMakeInstance_CreatedAtUpperLimit` — `crates/relay/tests/protocol.rs` the_information_document_describes_the_relay
- `TestInstance_JoinIsLoggedAgainstItsClaim` — Pending: ported in P2.

## lib_test.go

- `TestDispatchRedirectFrom` — `crates/relay/tests/protocol.rs` a_verified_custom_domain_and_the_canonical_host_both_serve_the_relay
- `TestDispatch_IgnoresInactiveInstances` — `crates/relay/tests/protocol.rs` a_deactivated_relay_closes_its_connections_and_stops_answering

## livekit_test.go

- `TestLivekitStore_ParticipantSeconds` — Pending: ported in P3.
- `TestLivekitStore_TokenHandler` — Pending: ported in P3.
- `TestLivekitStore_WebhookHandler` — Pending: ported in P3.

## main_test.go

- `TestMain` — Obsolete: `caravel_db::testing` creates a database per test binary.

## management_test.go

- `TestValidateJoinRequest_PaidRelay` — Pending: ported in P2.
- `TestValidateJoinRequest_PaidRelayTakesAnInvite` — Pending: ported in P2.
- `TestManagementStore_MembershipChangesAreIdempotent` — Pending: ported in P2.
- `TestManagementStore_BanPubkey` — Pending: ported in P2.
- `TestManagementStore_AllowPubkey` — Pending: ported in P2.
- `TestManagementStore_UnbanPubkey` — Pending: ported in P2.
- `TestManagementStore_UnallowPubkey` — Pending: ported in P2.
- `TestManagementStore_BanEvent` — Pending: ported in P2.
- `TestManagementStore_AllowEvent` — Pending: ported in P2.
- `TestManagementStore_BanCacheInvalidation` — Pending: ported in P2.
- `TestManagementStore_CreateRole` — Pending: ported in P2.
- `TestManagementStore_CreateRole_Duplicate` — Pending: ported in P2.
- `TestManagementStore_CreateRole_OmitsEmptyAndZero` — Pending: ported in P2.
- `TestManagementStore_CreateRole_InvalidColor` — Pending: ported in P2.
- `TestManagementStore_EditRole` — Pending: ported in P2.
- `TestManagementStore_AssignAndUnassignRole` — Pending: ported in P2.
- `TestManagementStore_AssignRole_UnknownRole` — Pending: ported in P2.
- `TestManagementStore_AssignAndUnassignMethod` — Pending: ported in P2.
- `TestManagementStore_GetMethodAssigneeItems` — Pending: ported in P2.
- `TestManagementStore_MemberMethods` — Pending: ported in P2.
- `TestManagementStore_RemoveMember_RemovesAssignedMethods` — Pending: ported in P2.
- `TestManagementStore_AssignMethod_UnknownMethod` — Pending: ported in P2.
- `TestManagementStore_MethodAssignments_OwnerAndSelf` — Pending: ported in P2.
- `TestManagementStore_AccessChecks` — Pending: ported in P2.
- `TestManagementStore_MethodCacheInvalidation` — Pending: ported in P2.
- `TestManagementStore_MemberCacheInvalidation` — Pending: ported in P2.
- `TestManagementStore_DeleteRole` — Pending: ported in P2.
- `TestManagementStore_DeleteRole_BroadcastsDeletion` — Pending: ported in P2.
- `TestManagementStore_PubkeyIsBanned_NotBanned` — Pending: ported in P2.
- `TestManagementStore_EventIsBanned_NotBanned` — Pending: ported in P2.
- `TestManagementStore_CreateClaim` — Pending: ported in P2.
- `TestManagementStore_CreateClaim_Idempotent` — Pending: ported in P2.
- `TestManagementStore_CreateClaim_Empty` — Pending: ported in P2.
- `TestManagementStore_GetClaims_ReturnsAll` — Pending: ported in P2.
- `TestManagementStore_DeleteClaim` — Pending: ported in P2.
- `TestManagementStore_DeleteClaim_RemovesAllMatching` — Pending: ported in P2.
- `TestManagementStore_CreateClaim_ValidatesJoinRequest` — Pending: ported in P2.
- `TestManagementStore_Enable_WiresListBannedEvents` — Pending: ported in P2.
- `TestManagementStore_Enable_PerMethodAuthorization` — Pending: ported in P2.
- `TestManagementStore_Enable_SupportedMethodsPerPubkey` — Pending: ported in P2.
- `TestManagementStore_ClaimLog` — Pending: ported in P2.
- `TestManagementStore_ClaimLog_PredatesLogging` — Pending: ported in P2.

## migrations_test.go

- `TestRunMigrations_RunsOnceInOrder` — Obsolete: sqlx runs the migrations.
- `TestRunMigrations_FailureAbortsAndRetries` — Obsolete: sqlx runs the migrations.
- `TestRemoveConfigRoles` — Obsolete: there are no config files.
- `TestRunMigrations_FailureRollsBack` — Obsolete: sqlx runs the migrations.
- `TestImportSqlite` — Obsolete: the one-time import replaces zooid migrations.
- `TestImportSqlite_UnparseableConfigAborts` — Obsolete: the one-time import replaces zooid migrations.
- `TestImportSqlite_NoDatabase` — Obsolete: the one-time import replaces zooid migrations.

## notify_test.go

- `TestNotifyStore_Set` — Pending: ported in P3.
- `TestNotifyStore_SetReplaces` — Pending: ported in P3.
- `TestNotifyStore_SetRequiresAMessage` — Pending: ported in P3.
- `TestNotifyStore_GetIsPerPubkey` — Pending: ported in P3.
- `TestNotifyStore_GetSkipsAnExpiredMessage` — Pending: ported in P3.
- `TestNotifyStore_GetKeepsAnUnexpiredMessage` — Pending: ported in P3.
- `TestNotifyStore_Clear` — Pending: ported in P3.
- `TestNotifyStore_ClearIsIdempotent` — Pending: ported in P3.
- `TestNotifyStore_NotificationsAreNotServed` — Pending: ported in P3.

## push_test.go

- `TestPushManager_ValidatePushSubscription_RejectsLocalCallback` — Pending: ported in P3.
- `TestPushManager_ValidatePushSubscription_AcceptsPublicCallback` — Pending: ported in P3.
- `TestPushManager_ValidatePushSubscription_RelayTagNormalization` — Pending: ported in P3.
- `TestPushManager_Enable_ClientRefusesToDialLoopback` — Pending: ported in P3.

## tenancy_test.go

- `TestEventStore_TenantIsolation` — `crates/core/tests/events.rs` relays_never_see_each_others_events
- `TestEventStore_ReplaceEvent_TenantIsolation` — `crates/core/tests/events.rs` a_replaceable_event_keeps_one_newest_version_per_slot
- `TestEventStore_ReplaceEvent_Concurrent` — `crates/core/tests/events.rs` racing_versions_of_a_slot_leave_the_newest
- `TestEventStore_TieOrder` — `crates/core/tests/events.rs` limits_cap_results_and_ties_break_on_id
- `TestEventStore_SaveEvent_RejectsNul` — `crates/core/tests/events.rs` events_with_a_nul_character_are_not_storable

## transfer_test.go

- `TestExportImportRoundTrip` — Pending: ported in P3.
- `TestImportEventsIsIdempotent` — Pending: ported in P3.
- `TestImportEventsSkipsBadLines` — Pending: ported in P3.
- `TestValidateImportEvent` — Pending: ported in P3.
- `TestImportEventsRebuildsGroupState` — Pending: ported in P3.
- `TestImportEventsAdmitsJoinRequests` — Pending: ported in P3.
- `TestImportEventsAddsToAnExistingGroup` — Pending: ported in P3.
- `TestImportEventsLeavesUncreatedGroupAlone` — Pending: ported in P3.
- `TestImportEventsDeletesDeletedGroup` — Pending: ported in P3.
- `TestImportEventsKeepsGroupRecreatedAfterDelete` — Pending: ported in P3.

## util_test.go

- `TestValidateNIP98Auth_AcceptsFreshEvent` — `crates/api/tests/auth.rs` a_signature_buys_a_session_until_it_is_deleted
- `TestValidateNIP98Auth_RejectsStaleEvent` — `crates/api/tests/auth.rs` a_session_signature_must_be_for_this_request_and_fresh
- `TestValidateNIP98Auth_RejectsFutureEvent` — `crates/api/tests/auth.rs` a_session_signature_must_be_for_this_request_and_fresh
- `TestIsInternalEvent_BlobDescriptor` — `crates/core/tests/relay_policy.rs` kinds_the_relay_writes_itself_are_refused
