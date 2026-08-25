*** Settings ***
Documentation       Check that one can create a batch of entities where some will succeed and others will fail

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entity
Suite Teardown      Delete Initial Entities


*** Test Cases ***
003_02_01 Create A Batch Of Two Valid Entities And One Invalid Entity
    [Documentation]    Check that one can create a batch of two valid entities and one invalid entity
    [Tags]    be-create    5_6_7
    ${first_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${first_entity_id}
    ${second_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${second_entity_id}
    # TODO: Use Load Test Sample keyword instead
    ${first_entity}=    Load Entity    building-minimal.jsonld    ${first_entity_id}
    ${second_entity}=    Load Entity    building-minimal.jsonld    ${second_entity_id}
    ${already_existing_entity}=    Load Entity    building-minimal.jsonld    ${existing_entity_id}
    @{entities_to_be_created}=    Create List    ${first_entity}    ${second_entity}    ${already_existing_entity}

    ${response}=    Batch Create Entities    @{entities_to_be_created}

    Check Response Status Code    207    ${response.status_code}
    @{expected_successful_entities_ids}=    Create List    ${first_entity_id}    ${second_entity_id}
    @{expected_failed_entities_ids}=    Create List    ${existing_entity_id}
    &{expected_batch_operation_result}=    Create Batch Operation Result
    ...    ${expected_successful_entities_ids}
    ...    ${expected_failed_entities_ids}
    Check Response Status Code    207    ${response.status_code}
    Check Response Body Containing Batch Operation Result    ${expected_batch_operation_result}    ${response.json()}
    ${expected_entities_ids}=    Catenate    SEPARATOR=,    @{expected_successful_entities_ids}
    ${response1}=    Query Entities
    ...    entity_ids=${expected_entities_ids}
    ...    entity_types=Building
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    @{created_entities}=    Create List    ${first_entity}    ${second_entity}
    Check Created Resources Set To    ${created_entities}    ${response1.json()}


*** Keywords ***
Setup Initial Entity
    ${existing_entity_id}=    Generate Random Building Entity Id
    ${create_response}=    Create Entity    building-minimal.jsonld    ${existing_entity_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${existing_entity_id}

Delete Initial Entities
    @{entities_ids_to_be_deleted}=    Create List    ${first_entity_id}    ${second_entity_id}    ${existing_entity_id}
    Batch Delete Entities    entities_ids_to_be_deleted=@{entities_ids_to_be_deleted}
