*** Settings ***
Documentation       Check that you can merge a batch of entities

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entities
Test Teardown       Delete Initial Entities
Test Template       Batch Merge Entity Scenarios


*** Variables ***
${entity_payload_filename}=     merge/building-merge-data.jsonld


*** Test Cases ***    FILENAME    MERGE_FRAGMENT_FILENAME
057_01_01 MergeAnEmptyEntity
    [Tags]    be-merge    5_6_17    since_v1.6.1
    building-minimal.jsonld    batch-merge/two-buildings-non-edited.jsonld
057_01_02 MergeSimpleProperties
    [Tags]    be-merge    5_6_17    since_v1.6.1
    building-simple-attributes-second.jsonld    batch-merge/two-buildings-attributes-edited.jsonld
057_01_03 MergeSimpleRelationship
    [Tags]    be-merge    5_6_17    since_v1.6.1
    building-relationship.jsonld    batch-merge/two-buildings-with-relationship.jsonld
057_01_04 MergePropertyWithPartialData
    [Tags]    be-merge    5_6_17    since_v1.6.1
    merge/building-only-airQualityLevel-value.jsonld    batch-merge/two-buildings-with-changed-airQualityLevel.jsonld


*** Keywords ***
Batch Merge Entity Scenarios
    [Documentation]    Check that you can merge a batch of entities
    [Arguments]    ${filename}    ${expectation_filename}
    ${first_entity}=    Load Entity    ${filename}    ${first_entity_id}
    ${second_entity}=    Load Entity    ${filename}    ${second_entity_id}
    @{entities_ids_to_be_merged}=    Create List    ${first_entity_id}    ${second_entity_id}
    @{entities_to_be_merged}=    Create List    ${first_entity}    ${second_entity}
    ${response}=    Batch Merge Entities    @{entities_to_be_merged}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}

    ${expected_entities_ids}=    Catenate    SEPARATOR=,    @{entities_ids_to_be_merged}
    ${response1}=    Query Entities
    ...    entity_ids=${expected_entities_ids}
    ...    entity_types=Building
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Response Body Containing List Containing Entity Elements With Different Types
    ...    filename=${expectation_filename}
    ...    entities_representation_ids=${entities_ids_to_be_merged}
    ...    response_body=${response1.json()}
    ...    ignore_core_context_version=${True}

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
