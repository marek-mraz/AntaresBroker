*** Settings ***
Documentation       Check that one can delete a batch of entities where some will succeed and others will fail

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entity


*** Test Cases ***
006_02_01 Delete A Batch Of Non-existing And Existing Entities
    [Documentation]    Check that one can delete a batch of non-existing and existing entities
    [Tags]    be-delete    5_6_10
    ${new_entity_id}=    Generate Random Building Entity Id
    @{entities_ids_to_be_deleted}=    Create List    ${existing_entity_id}    ${new_entity_id}

    ${response}=    Batch Delete Entities    entities_ids_to_be_deleted=@{entities_ids_to_be_deleted}

    Check Response Status Code    207    ${response.status_code}
    @{expected_successful_entities_ids}=    Create List    ${existing_entity_id}
    @{expected_failed_entities_ids}=    Create List    ${new_entity_id}
    Set Test Variable    ${expected_successful_entities_ids}

    &{response1}=    Create Batch Operation Result
    ...    ${expected_successful_entities_ids}
    ...    ${expected_failed_entities_ids}
    Check Response Status Code    207    ${response.status_code}
    Check Response Body Containing Batch Operation Result    ${response1}    ${response.json()}

    ${expected_entities_ids}=    Catenate    SEPARATOR=,    @{expected_successful_entities_ids}
    ${response2}=    Query Entities
    ...    entity_ids=${expected_entities_ids}
    ...    entity_types=Building
    ...    context=${ngsild_test_suite_context}
    Check SUT Not Containing Resources    ${response2.json()}


*** Keywords ***
Setup Initial Entity
    ${existing_entity_id}=    Generate Random Building Entity Id
    ${create_response}=    Create Entity    building-simple-attributes.jsonld    ${existing_entity_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Test Variable    ${existing_entity_id}
