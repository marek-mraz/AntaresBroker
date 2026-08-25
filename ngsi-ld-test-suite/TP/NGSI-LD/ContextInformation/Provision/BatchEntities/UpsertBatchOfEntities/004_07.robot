*** Settings ***
Documentation       Check that one can upsert a batch of entities where two have the same id

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Entities


*** Test Cases ***
004_07_01 Upsert A Batch Of Three Valid Entities Where Two Have The Same Id
    [Documentation]    Check that one can upsert a batch of where two have the same id
    [Tags]    be-upsert    5_6_8    5_5_11    since_v1.5.1
    ${first_entity_id}=    Generate Random Building Entity Id
    ${second_entity_id}=    Generate Random Building Entity Id
    ${first_entity}=    Load Entity    building-minimal.jsonld    ${first_entity_id}
    ${second_entity}=    Load Entity    building-minimal.jsonld    ${second_entity_id}
    @{entities_to_be_upserted}=    Create List    ${first_entity}    ${second_entity}    ${first_entity}

    ${response}=    Batch Upsert Entities    @{entities_to_be_upserted}

    @{expected_successful_entities_ids}=    Create List    ${first_entity_id}    ${second_entity_id}
    Set Test Variable    @{expected_successful_entities_ids}
    Check Response Status Code    201    ${response.status_code}
    Check Response Body Containing Array Of URIs set to    ${expected_successful_entities_ids}    ${response.json()}


*** Keywords ***
Delete Entities
    ${response}=    Batch Delete Entities
    ...    entities_ids_to_be_deleted=@{expected_successful_entities_ids}
