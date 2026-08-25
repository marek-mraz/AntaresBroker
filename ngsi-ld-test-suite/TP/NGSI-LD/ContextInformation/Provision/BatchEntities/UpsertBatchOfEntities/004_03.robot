*** Settings ***
Documentation       Check that one can upsert a batch of existing entities and they will be replaced

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entities
Test Teardown       Delete Initial Entities
Test Template       Batch Upsert Existing Entities Scenarios


*** Test Cases ***    FILENAME
004_03_01 EntityWithSimpleProperties
    [Tags]    be-upsert    5_6_8
    building-simple-attributes.jsonld
004_03_02 EntityWithSimpleRelationships
    [Tags]    be-upsert    5_6_8
    building-relationship.jsonld
004_03_03 EntityWithRelationshipsProperties
    [Tags]    be-upsert    5_6_8
    building-relationship-of-property.jsonld
004_03_04 EntityWithScope
    [Tags]    be-upsert    4_18    5_6_8
    building-with-one-scope.jsonld
004_03_05 EntityWithTypes
    [Tags]    be-upsert    4_16    5_6_8
    building-with-two-types.jsonld


*** Keywords ***
Batch Upsert Existing Entities Scenarios
    [Documentation]    Check that one can upsert a batch of existing entities
    [Arguments]    ${filename}
    ${first_existing_entity}=    Load Entity    ${filename}    ${first_existing_entity_id}
    ${second_existing_entity}=    Load Entity    ${filename}    ${second_existing_entity_id}
    @{entities_to_be_upserted}=    Create List    ${first_existing_entity}    ${second_existing_entity}

    ${response}=    Batch Upsert Entities    @{entities_to_be_upserted}

    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    @{upserted_entities_ids}=    Create List    ${first_existing_entity_id}    ${second_existing_entity_id}
    ${expected_updated_entities_ids}=    Catenate    SEPARATOR=,    @{upserted_entities_ids}
    ${response1}=    Query Entities
    ...    entity_ids=${expected_updated_entities_ids}
    ...    entity_types=Building
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Updated Resources Set To    ${entities_to_be_upserted}    ${response1.json()}

Setup Initial Entities
    ${first_existing_entity_id}=    Generate Random Building Entity Id
    ${create_response1}=    Create Entity    building-minimal.jsonld    ${first_existing_entity_id}
    Check Response Status Code    201    ${create_response1.status_code}
    Set Test Variable    ${first_existing_entity_id}
    ${second_existing_entity_id}=    Generate Random Building Entity Id
    ${create_response2}=    Create Entity    building-minimal.jsonld    ${second_existing_entity_id}
    Check Response Status Code    201    ${create_response2.status_code}
    Set Test Variable    ${second_existing_entity_id}

Delete Initial Entities
    @{entities_ids_to_be_deleted}=    Create List    ${first_existing_entity_id}    ${second_existing_entity_id}
    Batch Delete Entities    entities_ids_to_be_deleted=@{entities_ids_to_be_deleted}
