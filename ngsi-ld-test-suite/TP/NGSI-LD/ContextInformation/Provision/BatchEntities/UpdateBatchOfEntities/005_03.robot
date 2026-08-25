*** Settings ***
Documentation       Check that one can update a batch of entities where some will succeed and others will fail

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Initial Entities


*** Variables ***
${entity_payload_filename}=     building-simple-attributes.jsonld
${update_fragment_filename}=    fragmentEntities/simple-attributes-relationship-of-property-fragment.json


*** Test Cases ***
005_03_01 Update A Batch Of Non-existing And Existing Entities
    [Documentation]    Check that one can update a batch of non-existing and existing entities
    [Tags]    be-update    5_6_9
    ${first_existing_entity}=    Load Entity
    ...    building-relationship-of-property.jsonld
    ...    ${first_existing_entity_id}
    ${second_existing_entity}=    Load Entity
    ...    building-relationship-of-property.jsonld
    ...    ${second_existing_entity_id}
    ${new_entity_id}=    Generate Random Building Entity Id
    ${new_entity}=    Load Entity    building-relationship-of-property.jsonld    ${new_entity_id}
    @{entities_to_be_updated}=    Create List    ${first_existing_entity}    ${second_existing_entity}    ${new_entity}

    ${response}=    Batch Update Entities    @{entities_to_be_updated}

    @{expected_successful_entities_ids}=    Create List    ${first_existing_entity_id}    ${second_existing_entity_id}
    Set Suite Variable    @{expected_successful_entities_ids}
    @{expected_failed_entities_ids}=    Create List    ${new_entity_id}
    &{expected_batch_operation_result}=    Create Batch Operation Result
    ...    ${expected_successful_entities_ids}
    ...    ${expected_failed_entities_ids}
    Check Response Status Code    207    ${response.status_code}
    Check Response Body Containing Batch Operation Result    ${expected_batch_operation_result}    ${response.json()}

    ${first_created_entity}=    Load Test Sample    entities/${entity_payload_filename}    ${first_existing_entity_id}
    ${second_created_entity}=    Load Test Sample
    ...    entities/${entity_payload_filename}
    ...    ${second_existing_entity_id}
    ${update_fragment}=    Load Test Sample    entities/${update_fragment_filename}
    ${first_updated_entity}=    Upsert Element In Entity    ${first_created_entity}    ${update_fragment}
    ${second_updated_entity}=    Upsert Element In Entity    ${second_created_entity}    ${update_fragment}
    @{updated_entities}=    Create List    ${first_updated_entity}    ${second_updated_entity}
    ${expected_entities_ids}=    Catenate    SEPARATOR=,    @{expected_successful_entities_ids}

    ${response1}=    Query Entities
    ...    entity_ids=${expected_entities_ids}
    ...    entity_types=Building
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Updated Resources Set To    ${updated_entities}    ${response1.json()}


*** Keywords ***
Setup Initial Entities
    ${first_existing_entity_id}=    Generate Random Building Entity Id
    ${create_response1}=    Create Entity    ${entity_payload_filename}    ${first_existing_entity_id}
    Check Response Status Code    201    ${create_response1.status_code}
    Set Suite Variable    ${first_existing_entity_id}
    ${second_existing_entity_id}=    Generate Random Building Entity Id
    ${create_response2}=    Create Entity    ${entity_payload_filename}    ${second_existing_entity_id}
    Check Response Status Code    201    ${create_response2.status_code}
    Set Suite Variable    ${second_existing_entity_id}

Delete Initial Entities
    Batch Delete Entities    entities_ids_to_be_deleted=@{expected_successful_entities_ids}
