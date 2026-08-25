*** Settings ***
Documentation       Check that you can update types of entities in a batch update operation

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entities
Test Teardown       Delete Initial Entities
Test Template       Batch Update Entity Scenarios


*** Variables ***
${entity_payload_filename}=     building-minimal.jsonld


*** Test Cases ***    FILENAME    UPDATE_FRAGMENT_FILENAME
005_05_01 EntityWithNewType
    [Tags]    be-update    5_6_9    4_16
    building-minimal-with-new-type.jsonld    fragmentEntities/building-two-types-fragment.jsonld


*** Keywords ***
Batch Update Entity Scenarios
    [Documentation]    Check that you can update types of entities in a batch update operation
    [Arguments]    ${filename}    ${update_fragment_filename}
    ${first_entity}=    Load Entity    ${filename}    ${first_entity_id}
    ${second_entity}=    Load Entity    ${filename}    ${second_entity_id}
    @{entities_ids_to_be_updated}=    Create List    ${first_entity_id}    ${second_entity_id}
    @{entities_to_be_updated}=    Create List    ${first_entity}    ${second_entity}
    ${response}=    Batch Update Entities    @{entities_to_be_updated}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${first_created_entity}=    Load Test Sample    entities/${entity_payload_filename}    ${first_entity_id}
    ${second_created_entity}=    Load Test Sample    entities/${entity_payload_filename}    ${second_entity_id}
    ${update_fragment}=    Load Test Sample    entities/${update_fragment_filename}
    ${first_updated_entity}=    Upsert Element In Entity    ${first_created_entity}    ${update_fragment}
    ${second_updated_entity}=    Upsert Element In Entity    ${second_created_entity}    ${update_fragment}
    @{updated_entities}=    Create List    ${first_updated_entity}    ${second_updated_entity}
    ${expected_entities_ids}=    Catenate    SEPARATOR=,    @{entities_ids_to_be_updated}
    ${response1}=    Query Entities
    ...    entity_ids=${expected_entities_ids}
    ...    entity_types=Building
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Updated Resources Set To    ${updated_entities}    ${response1.json()}

Setup Initial Entities
    ${first_entity_id}=    Generate Random Building Entity Id
    ${second_entity_id}=    Generate Random Building Entity Id
    Create Entity    ${entity_payload_filename}    ${first_entity_id}
    Create Entity    ${entity_payload_filename}    ${second_entity_id}
    Set Test Variable    ${first_entity_id}
    Set Test Variable    ${second_entity_id}

Delete Initial Entities
    @{entities_ids_to_be_deleted}=    Create List    ${first_entity_id}    ${second_entity_id}
    Batch Delete Entities    entities_ids_to_be_deleted=@{entities_ids_to_be_deleted}
