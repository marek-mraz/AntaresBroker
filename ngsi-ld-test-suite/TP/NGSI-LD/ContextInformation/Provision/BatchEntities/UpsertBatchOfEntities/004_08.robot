*** Settings ***
Documentation       Check that you can upsert a batch of existing entities with new types and they will be replaced

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entities
Test Teardown       Delete Initial Entities
Test Template       Batch Upsert Existing Entities Scenarios


*** Test Cases ***    FILENAME
004_08_01 EntityWithNewTypes
    [Tags]    be-upsert    5_6_8    4_16
    building-minimal-with-new-types.jsonld


*** Keywords ***
Batch Upsert Existing Entities Scenarios
    [Documentation]    Check that you can upsert a batch of existing entities with new types
    [Arguments]    ${filename}
    ${first_existing_entity}=    Load Entity    ${filename}    ${first_existing_entity_id}
    ${second_existing_entity}=    Load Entity    ${filename}    ${second_existing_entity_id}
    @{entities_to_be_upserted}=    Create List    ${first_existing_entity}    ${second_existing_entity}
    ${response}=    Batch Upsert Entities    @{entities_to_be_upserted}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    @{upserted_entities_ids}=    Create List    ${first_existing_entity_id}    ${second_existing_entity_id}
    ${expected_updated_entities_ids}=    Catenate    SEPARATOR=,    @{upserted_entities_ids}
    ${response1}=    Query Entities
    ...    entity_ids=${expected_updated_entities_ids}
    ...    entity_types=Building
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Updated Resources Set To    ${entities_to_be_upserted}    ${response1.json()}

Setup Initial Entities
    ${first_existing_entity_id}=    Generate Random Building Entity Id
    ${second_existing_entity_id}=    Generate Random Building Entity Id
    Create Entity    building-minimal.jsonld    ${first_existing_entity_id}
    Create Entity    building-minimal.jsonld    ${second_existing_entity_id}
    Set Test Variable    ${first_existing_entity_id}
    Set Test Variable    ${second_existing_entity_id}

Delete Initial Entities
    @{entities_ids_to_be_deleted}=    Create List    ${first_existing_entity_id}    ${second_existing_entity_id}
    Batch Delete Entities    entities_ids_to_be_deleted=@{entities_ids_to_be_deleted}
