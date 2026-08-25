*** Settings ***
Documentation       Verify that, when one has an inclusive registration on a Context Broker, one entity on it and
...                 another on a Context Source, if one queries the Context Broker the query gets forwarded to the
...                 Context Source correctly

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown       Delete Created Entity And Registration And Stop Context Source Mock Server


*** Variables ***
${entity_payload_filename}              vehicle-simple-attributes.jsonld
${entity_payload_filename2}             vehicle-simple-attributes-second.json
${registration_id_prefix}               urn:ngsi-ld:Registration:
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-complete.jsonld


*** Test Cases ***
D011_01_03_inc Query The Context Broker With Type And Attribute
    [Documentation]    Check that if one queries for attribute present in an entity on a Context Source, only that
    ...    entity gets returned
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    additive-inclusive    4_3_6_2    5_7_2
    ${serialized_entity}=    Load Entity As Serialized Array    ${entity_payload_filename2}    ${second_entity_id}
    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=Vehicle    200    ${serialized_entity}

    ${response}=    Query Entities    entity_types=Vehicle    attrs=isParked2    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    @{entities_id}=    Create List    ${second_entity_id}
    Check Response Body Containing Entities URIS set to    ${entities_id}    ${response.json()}


*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${first_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${first_entity_id}
    ${response}=    Create Entity    ${entity_payload_filename}    ${first_entity_id}
    Check Response Status Code    201    ${response.status_code}

    ${second_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${second_entity_id}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ${response2}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response2.status_code}
    Start Context Source Mock Server

Delete Created Entity And Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Entity    ${first_entity_id}
    Stop Context Source Mock Server
