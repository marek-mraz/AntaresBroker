*** Settings ***
Documentation       Check that one can delete the default attribute instance using NGSI-LD Null in a Merge Entity operation

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entity
Test Teardown       Delete Initial Entity
Test Template       Merge Entity Scenarios


*** Variables ***
${entity_payload_filename}=     building-different-default-attributes-instances-and-types.jsonld


*** Test Cases ***    FILENAME    EXPECTATION_FILENAME
056_05_01 Delete A Property Instance
    [Tags]    e-merge    5_6_17    6_5_3_4    4_5_5    since_v1.6.1
    fragmentEntities/ngsild-null/building-null-property.jsonld    ngsild-null/building-deleted-property-default-instance.jsonld
056_05_02 Delete A Relationship Instance
    [Tags]    e-merge    5_6_17    6_5_3_4    4_5_5    since_v1.6.1
    fragmentEntities/ngsild-null/building-null-relationship.jsonld    ngsild-null/building-deleted-relationship-default-instance.jsonld
056_05_03 Delete A GeoProperty Instance
    [Tags]    e-merge    5_6_17    6_5_3_4    4_5_5    since_v1.6.1
    fragmentEntities/ngsild-null/building-null-geoproperty.jsonld    ngsild-null/building-deleted-geoproperty-default-instance.jsonld
056_05_04 Delete A LanguageProperty Instance
    [Tags]    e-merge    5_6_17    6_5_3_4    4_5_18    4_5_5    since_v1.6.1
    fragmentEntities/ngsild-null/building-null-languageproperty.jsonld    ngsild-null/building-deleted-languageproperty-default-instance.jsonld


*** Keywords ***
Merge Entity Scenarios
    [Documentation]    Check that one can delete the default attribute instance using NGSI-LD Null in a Merge Entity operation
    [Arguments]    ${filename}    ${expectation_filename}
    ${response}=    Merge Entity
    ...    entity_id=${entity_id}
    ...    entity_filename=${filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}

    ${response1}=    Retrieve Entity
    ...    id=${entity_id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Response Body Containing Entity element
    ...    expectation_filename=${expectation_filename}
    ...    entity_id=${entity_id}
    ...    response_body=${response1.json()}

Setup Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}
    Set Test Variable    ${entity_id}
    Check Response Status Code    201    ${response.status_code}

Delete Initial Entity
    Delete Entity    ${entity_id}
