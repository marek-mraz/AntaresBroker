*** Settings ***
Documentation       Check that one can update a batch of entities

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entities
Test Teardown       Delete Initial Entities
Test Template       Batch Update Entity Scenarios


*** Variables ***
${entity_payload_filename}=     building-simple-attributes.jsonld


*** Test Cases ***    FILENAME    UPDATE_FRAGMENT_FILENAME
005_01_01 EntityWithSimpleProperties
    [Tags]    be-update    5_6_9
    building-simple-attributes.jsonld    fragmentEntities/empty-fragment.json
005_01_02 EntityWithSimpleRelationships
    [Tags]    be-update    5_6_9
    building-relationship.jsonld    fragmentEntities/locatedAt-fragment.json
005_01_03 EntityWithRelationshipsProperties
    [Tags]    be-update    5_6_9
    building-relationship-of-property.jsonld    fragmentEntities/airQualityLevel-with-relationship-fragment.json


*** Keywords ***
Batch Update Entity Scenarios
    [Documentation]    Check that one can update a batch of entities
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
    ${create_response1}=    Create Entity    ${entity_payload_filename}    ${first_entity_id}
    Check Response Status Code    201    ${create_response1.status_code}
    Set Test Variable    ${first_entity_id}
    ${second_entity_id}=    Generate Random Building Entity Id
    ${create_response2}=    Create Entity    ${entity_payload_filename}    ${second_entity_id}
    Check Response Status Code    201    ${create_response2.status_code}
    Set Test Variable    ${second_entity_id}

Delete Initial Entities
    @{entities_ids_to_be_deleted}=    Create List    ${first_entity_id}    ${second_entity_id}
    Batch Delete Entities    entities_ids_to_be_deleted=@{entities_ids_to_be_deleted}
