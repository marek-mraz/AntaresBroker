*** Settings ***
Documentation       Check that one can delete a batch of entities with the same id

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entity


*** Test Cases ***
006_04_01 Delete A Batch Of Existing Entities With The Same Id
    [Documentation]    Check that one can delete a batch entities with the same id
    [Tags]    be-delete    5_6_10    5_5_11    since_v1.5.1
    ${new_entity_id}=    Generate Random Building Entity Id
    @{entities_ids_to_be_deleted}=    Create List    ${entity_id}    ${entity_id}

    ${response}=    Batch Delete Entities    entities_ids_to_be_deleted=@{entities_ids_to_be_deleted}

    Check Response Status Code    207    ${response.status_code}
    @{expected_successful_entities_ids}=    Create List    ${entity_id}
    @{expected_failed_entities_ids}=    Create List    ${entity_id}
    &{response1}=    Create Batch Operation Result
    ...    ${expected_successful_entities_ids}
    ...    ${expected_failed_entities_ids}
    Check Response Body Containing Batch Operation Result    ${response1}    ${response.json()}


*** Keywords ***
Setup Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    ${create_response}=    Create Entity    building-simple-attributes.jsonld    ${entity_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Test Variable    ${entity_id}
