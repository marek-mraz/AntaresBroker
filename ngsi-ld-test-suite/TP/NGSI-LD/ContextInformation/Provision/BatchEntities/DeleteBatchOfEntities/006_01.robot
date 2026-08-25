*** Settings ***
Documentation       Check that one can delete a batch of entities

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entities


*** Test Cases ***
006_01_01 Delete A Batch Of Entities
    [Documentation]    Check that one can delete a batch of entities
    [Tags]    be-delete    5_6_10
    ${response}=    Batch Delete Entities    entities_ids_to_be_deleted=@{entities_ids_to_be_deleted}

    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${expected_entities_ids}=    Catenate    SEPARATOR=,    @{entities_ids_to_be_deleted}
    ${response1}=    Query Entities
    ...    entity_ids=${expected_entities_ids}
    ...    entity_types=Building
    ...    context=${ngsild_test_suite_context}
    Check SUT Not Containing Resources    ${response1.json()}


*** Keywords ***
Setup Initial Entities
    ${first_entity_id}=    Generate Random Building Entity Id
    ${create_response1}=    Create Entity    building-simple-attributes.jsonld    ${first_entity_id}
    Check Response Status Code    201    ${create_response1.status_code}
    ${second_entity_id}=    Generate Random Building Entity Id
    ${create_response2}=    Create Entity    building-simple-attributes.jsonld    ${second_entity_id}
    Check Response Status Code    201    ${create_response2.status_code}
    @{entities_ids_to_be_deleted}=    Create List    ${first_entity_id}    ${second_entity_id}
    Set Test Variable    ${entities_ids_to_be_deleted}
