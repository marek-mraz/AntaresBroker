*** Settings ***
Documentation       Check that you can merge a batch of entities where some will succeed and others will fail

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Initial Entities


*** Variables ***
${entity_payload_filename}=     building-simple-attributes.jsonld
${merge_fragment_filename}=     fragmentEntities/simple-attributes-relationship-of-property-fragment.json
${entity_filename}=             building-relationship-of-property.jsonld


*** Test Cases ***
057_02_01 Merge A Batch Should Succeed For Existing Entities And Fail For Non Existing One By Returning 207 Status
    [Documentation]    Check that you can merge a batch of non-existing and existing entities
    [Tags]    be-merge    5_6_17    since_v1.6.1
    ${first_existing_entity}=    Load Entity
    ...    ${entity_filename}
    ...    ${first_existing_entity_id}
    ${second_existing_entity}=    Load Entity
    ...    ${entity_filename}
    ...    ${second_existing_entity_id}
    ${new_entity_id}=    Generate Random Building Entity Id
    ${new_entity}=    Load Entity    ${entity_filename}    ${new_entity_id}
    @{entities_to_be_merged}=    Create List    ${first_existing_entity}    ${second_existing_entity}    ${new_entity}

    ${response}=    Batch Merge Entities    @{entities_to_be_merged}
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
    ${merge_fragment}=    Load Test Sample    entities/${merge_fragment_filename}
    ${first_merged_entity}=    Upsert Element In Entity    ${first_created_entity}    ${merge_fragment}
    ${second_merged_entity}=    Upsert Element In Entity    ${second_created_entity}    ${merge_fragment}
    @{merged_entities}=    Create List    ${first_merged_entity}    ${second_merged_entity}
    ${expected_entities_ids}=    Catenate    SEPARATOR=,    @{expected_successful_entities_ids}

    ${response1}=    Query Entities
    ...    entity_ids=${expected_entities_ids}
    ...    entity_types=Building
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Updated Resources Set To    ${merged_entities}    ${response1.json()}


*** Keywords ***
Setup Initial Entities
    ${first_existing_entity_id}=    Generate Random Building Entity Id
    ${second_existing_entity_id}=    Generate Random Building Entity Id
    Create Entity    ${entity_payload_filename}    ${first_existing_entity_id}
    Create Entity    ${entity_payload_filename}    ${second_existing_entity_id}
    Set Suite Variable    ${first_existing_entity_id}
    Set Suite Variable    ${second_existing_entity_id}

Delete Initial Entities
    Batch Delete Entities    entities_ids_to_be_deleted=@{expected_successful_entities_ids}
