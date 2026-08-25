*** Settings ***
Documentation       Check that one can upsert a batch of entities where some will succeed and others will fail

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Entities


*** Test Cases ***
004_05_01 Upsert A Batch Of Two Valid Entities And One Invalid Entity
    [Documentation]    Check that one can upsert a batch of two valid entities and one invalid entity
    [Tags]    be-upsert    5_6_8
    ${first_entity_id}=    Generate Random Building Entity Id
    ${second_entity_id}=    Generate Random Building Entity Id
    ${third_entity_id}=    Generate Random Building Entity Id
    ${first_entity}=    Load Entity    building-minimal.jsonld    ${first_entity_id}
    ${second_entity}=    Load Entity    building-minimal.jsonld    ${second_entity_id}
    ${third_entity}=    Load Entity    building-minimal.jsonld    ${third_entity_id}
    ${invalid_entity}=    Delete Object From JSON    ${third_entity}    $.type

    @{entities_to_be_upserted}=    Create List    ${first_entity}    ${second_entity}    ${invalid_entity}

    ${response}=    Batch Upsert Entities    @{entities_to_be_upserted}

    Check Response Status Code    207    ${response.status_code}
    @{expected_successful_entities_ids}=    Create List    ${first_entity_id}    ${second_entity_id}
    Set Test Variable    ${expected_successful_entities_ids}
    @{expected_failed_entities_ids}=    Create List    ${third_entity_id}
    &{expected_batch_operation_result}=    Create Batch Operation Result
    ...    success=${expected_successful_entities_ids}
    ...    errors=${expected_failed_entities_ids}
    Check Response Body Containing Batch Operation Result    ${expected_batch_operation_result}    ${response.json()}
    ${expected_updated_entities_ids}=    Catenate    SEPARATOR=,    @{expected_successful_entities_ids}
    ${response1}=    Query Entities
    ...    entity_ids=${expected_updated_entities_ids}
    ...    entity_types=Building
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    @{upserted_entities}=    Create List    ${first_entity}    ${second_entity}
    Check Updated Resources Set To    ${upserted_entities}    ${response1.json()}


*** Keywords ***
Delete Entities
    ${response}=    Batch Delete Entities
    ...    entities_ids_to_be_deleted=@{expected_successful_entities_ids}
